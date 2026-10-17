//! pages.js (insert, delete, rotate, reorder, extract pages), blank.js `blankPdf` / `drawGrid` and pdf-lib's
//! document creation, page tree editing and page copying as pages.js uses them. Each operation takes the file's bytes
//! and gives new bytes; the original is never touched.
//!
//! The page tree is edited the way pdf-lib edits it (`insertLeafNode` / `removeLeafNode` with their Count updates and
//! pruning, so a moved nested tree ends up flat, as pdf-lib leaves it); pages copied from another file go through a
//! copier that follows pdf-lib's `PDFObjectCopier` (inherited attributes moved onto the page, /Parent dropped, every
//! referenced object copied once).

use crate::libdoc::{LibDoc, Ref, deflate};
use crate::obj::{Dict, Obj, Stream};
use cc_markup::i18n::t0;
use cc_markup::pages::{BlankSpec, clean_blank, grid_content, order_delete, order_insert, order_move, paper_metric, paper_size};
use serde_json::Value;

/// `PDFDocument.load(bytes, {updateMetadata: false})` as pages.js calls it.
pub fn load(bytes: &[u8]) -> Result<LibDoc, String> {
    let doc = LibDoc::parse(bytes)?;
    if doc.is_encrypted() {
        return Err(t0("save.err.encrypted"));
    }
    Ok(doc)
}

/// pdf-lib's date string `D:YYYYMMDDHHmmSSZ`.
pub fn pdf_date(now: f64) -> Obj {
    let (y, mo, d, h, mi, s, _) = cc_markup::js::civil(now);
    let year = if y < 0 { format!("-{:03}", y.unsigned_abs()) } else { format!("{y:04}") };
    Obj::lit(&format!("D:{year}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z"))
}

const PDF_LIB: &str = "pdf-lib (https://github.com/Hopding/pdf-lib)";

/// `PDFDocument.create()`: the page tree (1), the catalog (2) and the Info dictionary (3) pdf-lib starts with.
pub fn create(now: f64) -> LibDoc {
    let mut doc = LibDoc { header: (1, 7), ..LibDoc::default() };
    let pages = doc
        .register(Obj::Dict(Dict::new().with("Type", Obj::name("Pages")).with("Kids", Obj::Arr(Vec::new())).with("Count", Obj::Num(0.0))));
    let cat = doc.register(Obj::Dict(Dict::new().with("Type", Obj::name("Catalog")).with("Pages", Obj::Ref(pages.0, pages.1))));
    doc.root = Some(Obj::Ref(cat.0, cat.1));
    let info = doc.register(Obj::Dict(
        Dict::new()
            .with("Producer", Obj::hex_text(PDF_LIB))
            .with("Creator", Obj::hex_text(PDF_LIB))
            .with("CreationDate", pdf_date(now))
            .with("ModDate", pdf_date(now)),
    ));
    doc.info = Some(Obj::Ref(info.0, info.1));
    doc
}

/// The Info dictionary (made when missing, as `getInfoDict`).
pub fn info_mut(doc: &mut LibDoc) -> Option<&mut Dict> {
    let have = doc.info.as_ref().is_some_and(|i| doc.lookup(i).as_dict().is_some());
    if !have {
        let r = doc.register(Obj::Dict(Dict::new()));
        doc.info = Some(Obj::Ref(r.0, r.1));
    }
    match doc.info.clone() {
        Some(Obj::Ref(a, b)) => doc.get_mut((a, b)).and_then(Obj::as_dict_mut),
        _ => None,
    }
}

fn catalog_mut(doc: &mut LibDoc) -> Option<&mut Dict> {
    let r = doc.root_ref()?;
    doc.get_mut(r).and_then(Obj::as_dict_mut)
}

fn pages_root(doc: &LibDoc) -> Option<Ref> {
    let c = doc.root.as_ref().map(|r| doc.lookup(r))?.as_dict()?;
    c.get("Pages")?.as_ref()
}

fn dict_of(doc: &LibDoc, r: Ref) -> Option<&Dict> {
    doc.get(r).and_then(Obj::as_dict)
}

fn kind_of(doc: &LibDoc, r: Ref) -> Option<&'static str> {
    match dict_of(doc, r)?.get("Type").and_then(Obj::as_name) {
        Some("Pages") => Some("Pages"),
        Some("Page") => Some("Page"),
        _ => None,
    }
}

fn count_of(doc: &LibDoc, r: Ref) -> f64 {
    dict_of(doc, r).and_then(|d| d.get("Count")).map(|c| doc.lookup(c)).and_then(Obj::as_num).unwrap_or(f64::NAN)
}

/// The node's Kids (resolved).
fn kids_of(doc: &LibDoc, r: Ref) -> Vec<Obj> {
    dict_of(doc, r).and_then(|d| d.get("Kids")).map(|k| doc.lookup(k)).and_then(Obj::as_arr).map(<[Obj]>::to_vec).unwrap_or_default()
}

/// Change a node's Kids array in place (where it lives: in the node, or its own object).
fn edit_kids(doc: &mut LibDoc, r: Ref, f: impl FnOnce(&mut Vec<Obj>)) {
    let target = match dict_of(doc, r).and_then(|d| d.get("Kids")) {
        Some(Obj::Ref(a, b)) => Some((*a, *b)),
        _ => None,
    };
    match target {
        Some(k) => {
            if let Some(Obj::Arr(a)) = doc.get_mut(k) {
                f(a);
            }
        }
        None => {
            if let Some(d) = doc.get_mut(r).and_then(Obj::as_dict_mut) {
                let mut a = match d.get("Kids") {
                    Some(Obj::Arr(a)) => a.clone(),
                    _ => Vec::new(),
                };
                f(&mut a);
                d.set("Kids", Obj::Arr(a));
            }
        }
    }
}

/// `ascend`: the node and its parents, each Count moved by `delta`.
fn ascend_count(doc: &mut LibDoc, from: Ref, delta: f64) {
    let mut cur = Some(from);
    for _ in 0..64 {
        let Some(r) = cur else { break };
        let c = count_of(doc, r);
        let parent = dict_of(doc, r).and_then(|d| d.get("Parent")).and_then(Obj::as_ref);
        if let Some(d) = doc.get_mut(r).and_then(Obj::as_dict_mut) {
            d.set("Count", Obj::Num(c + delta));
        }
        cur = parent;
    }
}

fn insert_leaf(doc: &mut LibDoc, node: Ref, leaf: Ref, target: f64, depth: usize) -> Result<Option<Ref>, String> {
    if depth > 64 {
        return Err("Page tree too deep".into());
    }
    let kids = kids_of(doc, node);
    let count = count_of(doc, node);
    if target > count {
        return Err(format!("Invalid targetIndex specified: targetIndex={target} must be less than Count={count}"));
    }
    let mut remaining = target;
    for (idx, k) in kids.iter().enumerate() {
        if remaining == 0.0 {
            ascend_count(doc, node, 1.0);
            edit_kids(doc, node, |a| a.insert(idx.min(a.len()), Obj::Ref(leaf.0, leaf.1)));
            return Ok(None);
        }
        let Some(kr) = k.as_ref() else { continue };
        match kind_of(doc, kr) {
            Some("Pages") => {
                let kc = count_of(doc, kr);
                if kc > remaining {
                    return Ok(Some(insert_leaf(doc, kr, leaf, remaining, depth.saturating_add(1))?.unwrap_or(kr)));
                }
                remaining -= kc;
            }
            Some("Page") => remaining -= 1.0,
            _ => {}
        }
    }
    if remaining == 0.0 {
        ascend_count(doc, node, 1.0);
        edit_kids(doc, node, |a| a.push(Obj::Ref(leaf.0, leaf.1)));
        return Ok(None);
    }
    Err(format!("Failed to insertLeafNode at targetIndex={target} due to corrupt page tree"))
}

fn remove_leaf(doc: &mut LibDoc, node: Ref, target: f64, depth: usize) -> Result<(), String> {
    if depth > 64 {
        return Err("Page tree too deep".into());
    }
    let kids = kids_of(doc, node);
    let count = count_of(doc, node);
    if target >= count {
        return Err(format!("Invalid targetIndex specified: targetIndex={target} must be less than Count={count}"));
    }
    let mut remaining = target;
    for (idx, k) in kids.iter().enumerate() {
        let Some(kr) = k.as_ref() else { continue };
        match kind_of(doc, kr) {
            Some("Pages") => {
                let kc = count_of(doc, kr);
                if kc > remaining {
                    remove_leaf(doc, kr, remaining, depth.saturating_add(1))?;
                    if kids_of(doc, kr).is_empty() {
                        edit_kids(doc, node, |a| {
                            if idx < a.len() {
                                a.remove(idx);
                            }
                        });
                    }
                    return Ok(());
                }
                remaining -= kc;
            }
            Some("Page") => {
                if remaining == 0.0 {
                    ascend_count(doc, node, -1.0);
                    edit_kids(doc, node, |a| {
                        if idx < a.len() {
                            a.remove(idx);
                        }
                    });
                    return Ok(());
                }
                remaining -= 1.0;
            }
            _ => {}
        }
    }
    Err(format!("Failed to removeLeafNode at targetIndex={target} due to corrupt page tree"))
}

/// `doc.insertPage(index, page)` for a page object already in the document.
pub fn insert_page(doc: &mut LibDoc, index: usize, page: Ref) -> Result<(), String> {
    let root = pages_root(doc).ok_or("No page tree")?;
    let parent = insert_leaf(doc, root, page, index as f64, 0)?.unwrap_or(root);
    if let Some(d) = doc.get_mut(page).and_then(Obj::as_dict_mut) {
        d.set("Parent", Obj::Ref(parent.0, parent.1));
    }
    Ok(())
}

/// `doc.removePage(index)`.
pub fn remove_page(doc: &mut LibDoc, index: usize) -> Result<(), String> {
    let root = pages_root(doc).ok_or("No page tree")?;
    remove_leaf(doc, root, index as f64, 0)
}

/// `PDFPage.create(doc)` + `setSize(w, h)`: a new page object (not in the tree yet).
pub fn new_page(doc: &mut LibDoc, w: f64, h: f64) -> Ref {
    doc.register(Obj::Dict(
        Dict::new()
            .with("Type", Obj::name("Page"))
            .with("Parent", Obj::Ref(u32::MAX, 0))
            .with("Resources", Obj::Dict(Dict::new()))
            .with("MediaBox", Obj::nums(&[0.0, 0.0, w, h])),
    ))
}

/// `page.getWidth()` / `getHeight()` (the inherited media box, unnormalised) and `getRotation().angle`.
pub fn page_size(doc: &LibDoc, page: Ref) -> ([f64; 2], f64) {
    let Some(d) = dict_of(doc, page) else { return ([f64::NAN; 2], 0.0) };
    let mb: Vec<f64> = doc
        .inherited(d, "MediaBox")
        .and_then(Obj::as_arr)
        .map(|a| a.iter().map(|x| doc.lookup(x).as_num().unwrap_or(f64::NAN)).collect())
        .unwrap_or_default();
    let g = |i: usize| mb.get(i).copied().unwrap_or(f64::NAN);
    let rot = doc.inherited(d, "Rotate").and_then(Obj::as_num).unwrap_or(0.0);
    ([g(2) - g(0), g(3) - g(1)], rot)
}

/// `norm(deg)`: the nearest quarter turn in 0..360.
fn norm(deg: f64) -> f64 {
    ((cc_markup::js::round(deg / 90.0) * 90.0) % 360.0 + 360.0) % 360.0
}

fn set_rotation(doc: &mut LibDoc, page: Ref, deg: f64) {
    if let Some(d) = doc.get_mut(page).and_then(Obj::as_dict_mut) {
        d.set("Rotate", Obj::Num(deg));
    }
}

/// `relabel(lib, doc, labels, order)`.
fn relabel(doc: &mut LibDoc, labels: Option<&[String]>, order: &[Option<f64>]) {
    if let Some(c) = catalog_mut(doc) {
        c.delete("PageLabels");
    }
    let Some(labels) = labels.filter(|l| l.iter().any(|x| !x.is_empty())) else { return };
    let mut nums = Vec::new();
    for (i, old) in order.iter().enumerate() {
        let l = old.and_then(|o| labels.get(cc_markup::js::f2usize(o))).filter(|s| !s.is_empty()).cloned();
        let text = l.unwrap_or_else(|| (i.saturating_add(1)).to_string());
        nums.push(Obj::Num(i as f64));
        nums.push(Obj::Dict(Dict::new().with("P", Obj::hex_text(&text))));
    }
    if let Some(c) = catalog_mut(doc) {
        c.set("PageLabels", Obj::Dict(Dict::new().with("Nums", Obj::Arr(nums))));
    }
}

fn uniq_sorted(idx: &[f64], n: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for &i in idx {
        if i.fract() == 0.0 && i >= 0.0 && i < n as f64 {
            let u = cc_markup::js::f2usize(i);
            if !out.contains(&u) {
                out.push(u);
            }
        }
    }
    out.sort_unstable();
    out
}

/// `rotatePages(lib, bytes, indices, delta)`.
pub fn rotate_pages(bytes: &[u8], indices: &[f64], delta: f64) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let pages = doc.pages();
    let idx = uniq_sorted(indices, pages.len());
    if idx.is_empty() {
        return Err(t0("pages.err.none"));
    }
    for i in idx {
        let Some(&p) = pages.get(i) else { continue };
        let (_, rot) = page_size(&doc, p);
        set_rotation(&mut doc, p, norm(rot + delta));
    }
    Ok(doc.save())
}

/// `deletePages(lib, bytes, indices, {labels})`.
pub fn delete_pages(bytes: &[u8], indices: &[f64], labels: Option<&[String]>) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let n = doc.pages().len();
    let idx = uniq_sorted(indices, n);
    if idx.is_empty() {
        return Err(t0("pages.err.none"));
    }
    if idx.len() >= n {
        return Err(t0("pages.err.all"));
    }
    for &i in idx.iter().rev() {
        remove_page(&mut doc, i)?;
    }
    let idxf: Vec<f64> = idx.iter().map(|i| *i as f64).collect();
    let order: Vec<Option<f64>> = order_delete(n as f64, &idxf).into_iter().map(Some).collect();
    relabel(&mut doc, labels, &order);
    Ok(doc.save())
}

/// `movePages(lib, bytes, indices, to, {labels})` (the same bytes back when nothing moves).
pub fn move_pages(bytes: &[u8], indices: &[f64], to: f64, labels: Option<&[String]>) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let pages = doc.pages();
    let n = pages.len();
    let order = order_move(n as f64, indices, to);
    if order.iter().enumerate().all(|(i, v)| *v == i as f64) {
        return Ok(bytes.to_vec());
    }
    for i in (0..n).rev() {
        remove_page(&mut doc, i)?;
    }
    for (k, i) in order.iter().enumerate() {
        if let Some(&p) = pages.get(cc_markup::js::f2usize(*i)) {
            insert_page(&mut doc, k, p)?;
        }
    }
    let ord: Vec<Option<f64>> = order.into_iter().map(Some).collect();
    relabel(&mut doc, labels, &ord);
    Ok(doc.save())
}

/// pdf-lib's `PDFObjectCopier` from one document into another. pdf-lib remembers every dictionary, array and stream
/// it has copied by object identity (a direct object is identified here by the indirect object holding it and its
/// path inside it), and an object reached again while it is still being copied — a page reached through an
/// annotation's /P while its /Annots array is being copied — is the copy in progress: no new references are taken for
/// its contents, which keeps every object number as pdf-lib's. Such a re-reached copy only ever lands in the orphan
/// page copy pdf-lib makes for /P (outside the page tree); it is written empty here, where pdf-lib writes the shared
/// array (an invisible difference).
struct Copier<'a> {
    src: &'a LibDoc,
    map: std::collections::HashMap<Ref, Ref>,
    done: std::collections::HashMap<(Ref, String), Option<Obj>>,
}

type Id = Option<(Ref, String)>;

fn child(id: &Id, k: &str) -> Id {
    id.as_ref().map(|(r, p)| (*r, format!("{p}/{k}")))
}

impl Copier<'_> {
    fn copy(&mut self, dest: &mut LibDoc, o: &Obj, id: Id, depth: usize) -> Obj {
        if depth > 200 {
            return Obj::Null;
        }
        if let Obj::Ref(a, b) = o {
            let r = (*a, *b);
            if let Some(n) = self.map.get(&r) {
                return Obj::Ref(n.0, n.1);
            }
            let n = dest.next_ref();
            self.map.insert(r, n);
            if let Some(v) = self.src.get(r) {
                let v = v.clone();
                let c = self.copy(dest, &v, Some((r, String::new())), depth.saturating_add(1));
                dest.put(n, c);
            }
            return Obj::Ref(n.0, n.1);
        }
        let is_page = matches!(o, Obj::Dict(d) if d.get("Type").and_then(Obj::as_name) == Some("Page"));
        let container = matches!(o, Obj::Dict(_) | Obj::Arr(_) | Obj::Stream(_));
        if container
            && !is_page
            && let Some(key) = id.clone()
        {
            match self.done.get(&key) {
                Some(Some(v)) => return v.clone(),
                Some(None) => {
                    return match o {
                        Obj::Arr(_) => Obj::Arr(Vec::new()),
                        _ => Obj::Dict(Dict::new()),
                    };
                }
                None => {
                    self.done.insert(key, None);
                }
            }
        }
        let out = match o {
            Obj::Dict(d) => {
                let mut out = Dict::new();
                if is_page {
                    for (k, v, vid) in self.page_entries(d, &id) {
                        let c = self.copy(dest, &v, vid, depth.saturating_add(1));
                        out.set(&k, c);
                    }
                } else {
                    for (k, v) in &d.0 {
                        let c = self.copy(dest, v, child(&id, k), depth.saturating_add(1));
                        out.set(k, c);
                    }
                }
                Obj::Dict(out)
            }
            Obj::Arr(a) => Obj::Arr(
                a.iter().enumerate().map(|(i, x)| self.copy(dest, x, child(&id, &i.to_string()), depth.saturating_add(1))).collect(),
            ),
            Obj::Stream(s) => {
                let mut nd = Dict::new();
                for (k, v) in &s.dict.0 {
                    let c = self.copy(dest, v, child(&id, k), depth.saturating_add(1));
                    nd.set(k, c);
                }
                Obj::Stream(Box::new(Stream { dict: nd, data: s.data.clone() }))
            }
            x => x.clone(),
        };
        if container
            && !is_page
            && let Some(key) = id
        {
            self.done.insert(key, Some(out.clone()));
        }
        out
    }
    /// `copyPDFPage`: the page's entries with the inherited ones moved onto it and /Parent dropped, each with the
    /// identity of the object it came from.
    fn page_entries(&self, d: &Dict, id: &Id) -> Vec<(String, Obj, Id)> {
        let mut out: Vec<(String, Obj, Id)> = d.0.iter().map(|(k, v)| (k.clone(), v.clone(), child(id, k))).collect();
        for k in ["Resources", "MediaBox", "CropBox", "Rotate"] {
            if d.get(k).is_some() {
                continue;
            }
            let mut parent = d.get("Parent").and_then(Obj::as_ref);
            for _ in 0..64 {
                let Some(pr) = parent else { break };
                let Some(pd) = self.src.get(pr).and_then(Obj::as_dict) else { break };
                if let Some(v) = pd.get(k) {
                    if !matches!(v, Obj::Null) {
                        out.push((k.to_string(), v.clone(), Some((pr, format!("/{k}")))));
                    }
                    break;
                }
                parent = pd.get("Parent").and_then(Obj::as_ref);
            }
        }
        out.retain(|(k, _, _)| k != "Parent");
        out
    }
}

/// `doc.copyPages(src, indices)` → the new page objects (registered, not in the tree).
fn copy_pages(dest: &mut LibDoc, src: &LibDoc, indices: &[usize]) -> Vec<Ref> {
    let src_pages = src.pages();
    let mut cp = Copier { src, map: std::collections::HashMap::new(), done: std::collections::HashMap::new() };
    let mut out = Vec::new();
    for i in indices {
        let Some(&p) = src_pages.get(*i) else { continue };
        let Some(node) = src.get(p).cloned() else { continue };
        let c = cp.copy(dest, &node, Some((p, String::new())), 0);
        out.push(dest.register(c));
    }
    out
}

/// `insertPages(lib, bytes, srcBytes, at, {indices, labels})` → (bytes, count).
pub fn insert_pages(
    bytes: &[u8],
    src_bytes: &[u8],
    at: f64,
    indices: Option<&[f64]>,
    labels: Option<&[String]>,
) -> Result<(Vec<u8>, usize), String> {
    let mut doc = load(bytes)?;
    let src = load(src_bytes)?;
    let sn = src.pages().len();
    let idx: Vec<usize> = match indices {
        Some(ix) => uniq_sorted(ix, sn),
        None => (0..sn).collect(),
    };
    if idx.is_empty() {
        return Err(t0("pages.err.none"));
    }
    let copies = copy_pages(&mut doc, &src, &idx);
    let n = doc.pages().len();
    let a = cc_markup::js::max(0.0, cc_markup::js::min(n as f64, at));
    let a = if a.is_nan() { 0 } else { cc_markup::js::f2usize(a) };
    for (k, p) in copies.iter().enumerate() {
        insert_page(&mut doc, a.saturating_add(k), *p)?;
    }
    let order = order_insert(n as f64, a as f64, copies.len() as f64);
    relabel(&mut doc, labels, &order);
    Ok((doc.save(), copies.len()))
}

/// `drawGrid(lib, doc, page, grid, metric)`: the grid as a compressed content stream of its own, added to the
/// page (pdf-lib's `addContentStream`, which normalises the page first).
pub fn draw_grid(doc: &mut LibDoc, page: Ref, grid: &str, metric: bool) -> bool {
    let ([w, h], _) = page_size(doc, page);
    let text = grid_content(w, h, grid, metric);
    if text.is_empty() {
        return false;
    }
    let r = doc.register(Obj::Stream(Box::new(Stream {
        dict: Dict::new().with("Filter", Obj::name("FlateDecode")),
        data: deflate(text.as_bytes()),
    })));
    add_content_stream(doc, page, r);
    true
}

/// `PDFPageLeaf.addContentStream` (with `normalize`, no q/Q wrapping: pages made here and pages read do not wrap
/// for this call in pdf-lib either when it is the first normalisation of a created page; read pages get the wrap).
fn add_content_stream(doc: &mut LibDoc, page: Ref, r: Ref) {
    let Some(mut pd) = dict_of(doc, page).cloned() else { return };
    let mut contents: Vec<Obj> = match pd.get("Contents").cloned() {
        Some(Obj::Ref(a, b)) => match doc.get((a, b)) {
            Some(Obj::Stream(_)) => vec![Obj::Ref(a, b)],
            Some(Obj::Arr(x)) => x.clone(),
            _ => Vec::new(),
        },
        Some(Obj::Arr(x)) => x,
        _ => Vec::new(),
    };
    let res = doc.inherited(&pd, "Resources").and_then(|r| if let Obj::Dict(d) = r { Some(d.clone()) } else { None }).unwrap_or_default();
    let mut res = res;
    for k in ["Font", "XObject", "ExtGState"] {
        let v =
            res.get(k).map(|x| doc.lookup(x).clone()).and_then(|x| if let Obj::Dict(d) = x { Some(d) } else { None }).unwrap_or_default();
        res.set(k, Obj::Dict(v));
    }
    pd.set("Resources", Obj::Dict(res));
    if !pd.has("Annots") {
        pd.set("Annots", Obj::Arr(Vec::new()));
    }
    contents.push(Obj::Ref(r.0, r.1));
    pd.set("Contents", Obj::Arr(contents));
    doc.put(page, Obj::Dict(pd));
}

/// `insertBlank(lib, bytes, at, {size, labels, grid})`.
pub fn insert_blank(
    bytes: &[u8],
    at: f64,
    size: Option<[f64; 2]>,
    labels: Option<&[String]>,
    grid: Option<(&str, bool)>,
) -> Result<Vec<u8>, String> {
    let mut doc = load(bytes)?;
    let pages = doc.pages();
    let n = pages.len();
    let a = cc_markup::js::max(0.0, cc_markup::js::min(n as f64, at));
    let a = if a.is_nan() { 0 } else { cc_markup::js::f2usize(a) };
    let rf = if n > 0 { pages.get(a.saturating_sub(1)).copied() } else { None };
    let ([rw, rh], rrot) = rf.map_or(([612.0, 792.0], 0.0), |p| page_size(&doc, p));
    let wh = size.unwrap_or([rw, rh]);
    let pg = new_page(&mut doc, wh[0], wh[1]);
    insert_page(&mut doc, a, pg)?;
    if rf.is_some() && size.is_none() {
        set_rotation(&mut doc, pg, norm(rrot));
    }
    if let Some((g, metric)) = grid {
        draw_grid(&mut doc, pg, g, metric);
    }
    relabel(&mut doc, labels, &order_insert(n as f64, a as f64, 1.0));
    Ok(doc.save())
}

/// `extractPages(lib, bytes, indices)` at time `now`.
pub fn extract_pages(bytes: &[u8], indices: &[f64], now: f64) -> Result<Vec<u8>, String> {
    let src = load(bytes)?;
    let sn = src.pages().len();
    let mut idx: Vec<usize> = Vec::new();
    for &i in indices {
        if i.fract() == 0.0 && i >= 0.0 && i < sn as f64 {
            let u = cc_markup::js::f2usize(i);
            if !idx.contains(&u) {
                idx.push(u);
            }
        }
    }
    if idx.is_empty() {
        return Err(t0("pages.err.none"));
    }
    let mut doc = create(now);
    let copies = copy_pages(&mut doc, &src, &idx);
    for (k, p) in copies.iter().enumerate() {
        insert_page(&mut doc, k, *p)?;
    }
    let title = src
        .info
        .as_ref()
        .map(|i| src.lookup(i))
        .and_then(Obj::as_dict)
        .and_then(|d| d.get("Title"))
        .map(|t| src.lookup(t))
        .and_then(Obj::decode_text);
    if let Some(info) = info_mut(&mut doc) {
        if let Some(t) = title.filter(|t| !t.is_empty()) {
            info.set("Title", Obj::hex_text(&t));
        }
        info.set("Producer", Obj::hex_text("MarkupCraft"));
    }
    Ok(doc.save())
}

/// `blankPdf(lib, raw, {title})` at time `now`.
pub fn blank_pdf(raw: Option<&Value>, title: &str, now: f64) -> Vec<u8> {
    let spec: BlankSpec = clean_blank(raw);
    let mut doc = create(now);
    let [w, h] = paper_size(&spec);
    let metric = paper_metric(spec.paper);
    let mut i = 0.0;
    while i < spec.pages {
        let n = doc.pages().len();
        let pg = new_page(&mut doc, w, h);
        if insert_page(&mut doc, n, pg).is_ok() {
            draw_grid(&mut doc, pg, spec.grid, metric);
        }
        i += 1.0;
    }
    let note = format!("{{\"paper\":{},\"orient\":{},\"grid\":{}}}", json_str(spec.paper), json_str(spec.orient), json_str(spec.grid));
    if let Some(info) = info_mut(&mut doc) {
        if !title.is_empty() {
            info.set("Title", Obj::hex_text(title));
        }
        info.set("Producer", Obj::hex_text("MarkupCraft"));
        info.set("Creator", Obj::hex_text("MarkupCraft"));
        info.set(crate::BLANK_KEY, Obj::hex_text(&note));
    }
    doc.save()
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}
