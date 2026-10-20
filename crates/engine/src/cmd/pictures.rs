//! The picture commands (pictures-cmd.js, 0.5 WL.14): IMAGEATTACH, IMAGE, IMAGECLIP.
//!
//! A picture is an unnamed block "*I<n>" placed by an insert: its frame (a closed poly carrying `pic`) and the file's
//! name as text (cc-io-cad `small::picture_block`). The files stay with the drawing while it is open (`Doc::pictures`,
//! cc-cad `pictures`); they are not undoable and not saved (a DWG names the file).

use std::sync::Arc;

use cc_cad::entity::{Entity, Geo, num};
use cc_cad::pictures::{PicRec, add_picture, find_picture, picture_key, strip_ext};
use cc_cad::{Block, Doc, MODEL, js};
use cc_cad_geom::geom::{insert_matrix, m_apply};
use cc_geom::P2;
use serde_json::{Value, json};

use crate::engine::{Ctx, Opts, PickedFile, R, Reply};
use crate::input::Def;

/// What the window's picker prompt says while it is open (`app.pictures.pick`).
fn pick_prompt(c: &Ctx) -> String {
    c.t("app.pictures.pick")
}

/// `pictureFrame(block)`: the index of the frame entity (the poly carrying `pic`).
pub fn picture_frame(b: &Block) -> Option<usize> {
    b.entities.iter().position(|e| matches!(e.geo, Geo::Poly(_)) && js::truthy(e.get("pic")))
}

/// The frame's `pic` file name as the drawing names it.
fn pic_file(b: &Block) -> Option<String> {
    let i = picture_frame(b)?;
    let f = b.entities.get(i)?.get("pic")?.get("file");
    Some(f.filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default())
}

/// `freePictureName(doc)`: a free "*I<n>".
pub fn free_picture_name(doc: &Doc) -> String {
    let mut n: u64 = 1;
    while n < 10_000_000 && doc.blocks.contains_key(&format!("*I{n}")) {
        n = n.saturating_add(1);
    }
    format!("*I{n}")
}

/// `pictureBlock(name, pic, clip)` through cc-io-cad's port (frame + name text).
pub fn picture_block(name: &str, pic: &Value, clip: Option<&[P2]>) -> Block {
    let cl: Option<Vec<Value>> = clip.map(|c| c.iter().map(|q| json!({ "x": num(q.x), "y": num(q.y) })).collect());
    Block::from_value(&cc_io_cad::small::picture_block_json(name, pic, cl.as_deref()))
}

/// `pickFiles(c, multiple)`: the files the host's picker gave (none when the host cannot open files: said).
async fn pick_files(c: &Ctx, multiple: bool) -> R<Vec<PickedFile>> {
    match c.pick_pictures(pick_prompt(c), multiple).await? {
        Some(f) => Ok(f),
        None => {
            c.print(&c.t("pic.nobrowser"));
            Ok(Vec::new())
        }
    }
}

/// `addPicture(doc, name, bytes)` → the record, or the printed reason.
fn add(c: &Ctx, name: &str, bytes: &[u8]) -> Option<Arc<PicRec>> {
    let r = {
        let mut d = c.sh().doc.borrow_mut();
        let r = add_picture(&mut d.pictures, name, bytes);
        if r.is_ok() {
            d.picture_version = d.picture_version.saturating_add(1);
        }
        r
    };
    match r {
        Ok(rec) => Some(rec),
        Err(key) => {
            c.print(&c.tv(key, &[("file", name.to_string())]));
            None
        }
    }
}

pub async fn imageattach(c: Ctx) -> R {
    let files = pick_files(&c, false).await?;
    let Some(f) = files.into_iter().next() else { return Ok(()) };
    let Some(rec) = add(&c, &f.name, &f.bytes) else { return Ok(()) };
    let Some(at) = c.point(c.tv("pic.prompt.corner", &[("file", rec.name.clone())]), Opts::default()).await?.point() else { return Ok(()) };
    let rw = f64::from(rec.w);
    let width = c
        .distance(
            c.tv("pic.prompt.width", &[("w", rec.w.to_string()), ("h", rec.h.to_string())]),
            Opts { base: Some(at), default: Some(Def::Num(rw)), ..Opts::default() },
        )
        .await?;
    let w = match width {
        Reply::Num(n) if n > 0.0 => n,
        _ => rw,
    };
    let ang = c.angle(c.t("pic.prompt.angle"), Opts { base: Some(at), default: Some(Def::Num(0.0)), ..Opts::default() }).await?;
    let a = if let Reply::Num(n) = ang { n } else { 0.0 };
    let px = w / rw;
    let pic = json!({ "file": rec.name, "x": 0, "y": 0, "ux": num(px), "uy": 0, "vx": 0, "vy": num(px), "w": rec.w, "h": rec.h });
    let name = free_picture_name(&c.doc());
    let blk = picture_block(&name, &pic, None);
    let layer = c.doc().current.layer.clone();
    let e = Entity::from_value(
        &json!({ "type": "insert", "layer": layer, "color": null, "block": name, "x": num(at.x), "y": num(at.y), "sx": 1, "sy": 1, "rot": num(a) }),
    );
    c.change("IMAGEATTACH", |d| {
        d.define_block(&blk)?;
        d.add(e)
    })?;
    c.print(&c.tv("pic.attached", &[("file", rec.name.clone()), ("w", rec.w.to_string()), ("h", rec.h.to_string())]));
    Ok(())
}

pub async fn image(c: Ctx) -> R {
    // key → the file as the drawing names it (the last block naming that key wins, as a Map keeps it)
    let mut used: Vec<(String, String)> = Vec::new();
    for b in c.doc().blocks.values() {
        if let Some(f) = pic_file(b) {
            let k = picture_key(&f);
            match used.iter_mut().find(|(key, _)| *key == k) {
                Some(slot) => slot.1 = f,
                None => used.push((k, f)),
            }
        }
    }
    if used.is_empty() {
        c.print(&c.t("pic.none"));
        return Ok(());
    }
    let found = |c: &Ctx, f: &str| find_picture(&c.doc().pictures, f).is_some();
    let missing: Vec<String> = used.iter().filter(|(_, f)| !found(&c, f)).map(|(_, f)| f.clone()).collect();
    c.print(&c.tv("pic.list", &[("n", used.len().to_string()), ("missing", missing.len().to_string())]));
    for (_, f) in &used {
        let key = if found(&c, f) { "pic.row.loaded" } else { "pic.row.missing" };
        c.print(&format!("  {}", c.tv(key, &[("file", f.clone())])));
    }
    if missing.is_empty() {
        return Ok(());
    }
    let files = pick_files(&c, true).await?;
    let mut n = 0usize;
    for f in files {
        let k = picture_key(&f.name);
        let stem = strip_ext(&k).to_string();
        // the exact name, else a file whose name contains the one the drawing names (eTransmit and other exports add a prefix)
        let target = missing.iter().find(|m| picture_key(m) == k).or_else(|| {
            missing.iter().find(|m| {
                let mk = picture_key(m);
                let s = strip_ext(&mk);
                s.encode_utf16().count() >= 3 && stem.contains(s)
            })
        });
        let Some(target) = target.cloned() else {
            c.print(&c.tv("pic.unused", &[("file", f.name.clone())]));
            continue;
        };
        let Some(rec) = add(&c, &f.name, &f.bytes) else { continue };
        let tk = picture_key(&target);
        if tk != k {
            c.sh().doc.borrow_mut().pictures.insert(tk, rec);
        }
        n = n.saturating_add(1);
    }
    if n > 0 {
        let mut d = c.sh().doc.borrow_mut();
        d.picture_version = d.picture_version.saturating_add(1);
    }
    c.print(&c.tv("pic.added", &[("n", n.to_string())]));
    Ok(())
}

pub async fn imageclip(c: Ctx) -> R {
    let p = c.entity(c.t("pic.prompt.pick"), Opts::default()).await?;
    let Reply::Pick { id, .. } = p else { return Ok(()) };
    let pick = {
        let d = c.doc();
        d.get(id).cloned().and_then(|e| {
            let Geo::Insert(i) = &e.geo else { return None };
            let b = d.blocks.get(&i.block)?.clone();
            let fr = picture_frame(&b)?;
            Some((e, b, fr))
        })
    };
    let Some((rf, blk, fr)) = pick else {
        c.print(&c.t("pic.notpicture"));
        return Ok(());
    };
    let options = vec![c.opt("R", &c.t("opt.rectangular")), c.opt("P", &c.t("opt.polygonal")), c.opt("D", &c.t("opt.delete"))];
    let k = c.keyword(c.t("pic.prompt.clip"), Opts { options, default: Some(Def::Str("R".into())), ..Opts::default() }).await?;
    let o = match k {
        Reply::Opt(s) | Reply::Str(s) if !s.is_empty() => s,
        _ => return Ok(()),
    };
    let mut pts: Option<Vec<P2>> = None;
    if o == "R" {
        let Some(a) = c.point(c.t("pic.prompt.corner1"), Opts::default()).await?.point() else { return Ok(()) };
        let Some(b) = c.point(c.t("pic.prompt.corner2"), Opts { base: Some(a), ..Opts::default() }).await?.point() else { return Ok(()) };
        pts = Some(vec![a, P2::new(b.x, a.y), b, P2::new(a.x, b.y)]);
    } else if o == "P" {
        let Some(v) = crate::cmd::xref::polygon(&c, "pic.prompt.vertex", "pic.prompt.vertex_or_enter").await? else { return Ok(()) };
        pts = Some(v);
    }
    // the clip in the block's own coordinates (the reference may be moved, turned and scaled)
    let Geo::Insert(ins) = &rf.geo else { return Ok(()) };
    let m = insert_matrix(ins, &blk);
    let d = m[0] * m[3] - m[1] * m[2];
    let inv = [m[3] / d, -m[1] / d, -m[2] / d, m[0] / d, (m[2] * m[5] - m[3] * m[4]) / d, (m[1] * m[4] - m[0] * m[5]) / d];
    let local: Option<Vec<P2>> = pts.as_ref().map(|v| v.iter().map(|q| m_apply(&inv, *q)).collect());
    // a picture block other references share gets a copy of its own first (a clip belongs to one picture, as in the file)
    let name = {
        let doc = c.doc();
        let refs = |e: &Entity| matches!(&e.geo, Geo::Insert(i) if i.block == blk.name);
        let mut shared = 0usize;
        for sp in std::iter::once(MODEL).chain(doc.layouts.iter().map(|l| l.space)) {
            shared = shared.saturating_add(doc.space_entities(sp).filter(|e| refs(e)).count());
        }
        for b in doc.blocks.values() {
            shared = shared.saturating_add(b.entities.iter().filter(|e| refs(e)).count());
        }
        if shared > 1 { free_picture_name(&doc) } else { blk.name.clone() }
    };
    let mut plain = blk.entities.get(fr).and_then(|e| e.get("pic")).cloned().unwrap_or(Value::Null);
    if let Value::Object(o) = &mut plain {
        o.shift_remove("clipped");
    }
    let nb = picture_block(&name, &plain, local.as_deref());
    let mut ents: Vec<Entity> = nb.entities.into_iter().take(1).collect();
    ents.extend(blk.entities.iter().enumerate().filter(|(i, _)| *i != fr).map(|(_, e)| e.clone()));
    let mut b2 = Block::new(&name, nb.base, ents);
    b2.attdefs = nb.attdefs;
    let rid = rf.id;
    let renamed = name != blk.name;
    c.change("IMAGECLIP", |d| {
        d.define_block(&b2)?;
        if renamed && let Some(id) = rid {
            let mut e = rf.clone();
            if let Geo::Insert(i) = &mut e.geo {
                i.block = name.clone();
            }
            d.update(id, e)?;
        }
        Ok(())
    })?;
    c.print(&c.t(if pts.is_some() { "pic.clipped" } else { "pic.unclipped" }));
    Ok(())
}
