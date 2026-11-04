//! A folder through the browser (0.5 sheetset-panel.js `pickFolder`): the File System Access directory picker where
//! the browser has one (read and write: the set file is saved back in place), else an `<input webkitdirectory>` pick
//! (read only, as 0.5's `folderFromFiles`). The folder's files are read when it is picked (the Sheet Set panel reads
//! them as it needs them; the window's folder is a plain one), at most 6 folders deep, dot-folders left out, within
//! the caps below; a write goes into the copy at once and to the folder on disk behind it.

use std::rc::Rc;

use cc_ui_egui::{FolderSlot, FolderWrite, PickedFolder};
use js_sys::{Function, Promise, Reflect, Uint8Array};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// The most bytes and files a folder may bring (the wasm build's memory is 4 GB at most).
const MAX_BYTES: f64 = 1024.0 * 1024.0 * 1024.0;
const MAX_FILES: usize = 20_000;

fn get(o: &JsValue, k: &str) -> JsValue {
    Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED)
}
fn call0(o: &JsValue, k: &str) -> Result<JsValue, JsValue> {
    get(o, k).dyn_into::<Function>()?.call0(o)
}
fn call1(o: &JsValue, k: &str, a: &JsValue) -> Result<JsValue, JsValue> {
    get(o, k).dyn_into::<Function>()?.call1(o, a)
}
fn call2(o: &JsValue, k: &str, a: &JsValue, b: &JsValue) -> Result<JsValue, JsValue> {
    get(o, k).dyn_into::<Function>()?.call2(o, a, b)
}
async fn wait(v: Result<JsValue, JsValue>) -> Result<JsValue, JsValue> {
    JsFuture::from(v?.dyn_into::<Promise>()?).await
}
fn msg(e: &JsValue) -> String {
    let m = get(e, "message");
    m.as_string().unwrap_or_else(|| format!("{e:?}"))
}

/// A file's bytes (`file.arrayBuffer()`).
async fn bytes_of(file: &JsValue) -> Result<Vec<u8>, JsValue> {
    let buf = wait(call0(file, "arrayBuffer")).await?;
    Ok(Uint8Array::new(&buf).to_vec())
}

/// Every file under a directory handle: ("a/b.dwg", bytes), sorted, read in full.
async fn read_dir(dir: &JsValue, pre: &str, depth: usize, out: &mut Vec<(String, Vec<u8>)>, total: &mut f64) -> Result<(), String> {
    let it = call0(dir, "entries").map_err(|e| msg(&e))?;
    loop {
        let r = wait(call0(&it, "next")).await.map_err(|e| msg(&e))?;
        if get(&r, "done").is_truthy() {
            break;
        }
        let pair = get(&r, "value");
        let name = Reflect::get_u32(&pair, 0).ok().and_then(|v| v.as_string()).unwrap_or_default();
        let h = Reflect::get_u32(&pair, 1).unwrap_or(JsValue::UNDEFINED);
        if get(&h, "kind").as_string().as_deref() == Some("directory") {
            if depth < 6 && !name.starts_with('.') {
                Box::pin(read_dir(&h, &format!("{pre}{name}/"), depth + 1, out, total)).await?;
            }
            continue;
        }
        if out.len() >= MAX_FILES {
            return Err("the folder holds too many files".into());
        }
        let file = wait(call0(&h, "getFile")).await.map_err(|e| msg(&e))?;
        *total += get(&file, "size").as_f64().unwrap_or(0.0);
        if *total > MAX_BYTES {
            return Err("the folder is over 1 GB".into());
        }
        out.push((format!("{pre}{name}"), bytes_of(&file).await.map_err(|e| msg(&e))?));
    }
    Ok(())
}

/// Write one file through the directory handle ("a/b.dst": folders made as needed; ".." refused).
async fn write_file(dir: JsValue, path: String, data: Vec<u8>) -> Result<(), String> {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let Some((name, folders)) = parts.split_last() else { return Err("no name".into()) };
    let create = js_sys::Object::new();
    let _ = Reflect::set(&create, &JsValue::from_str("create"), &JsValue::TRUE);
    let mut d = dir;
    for p in folders {
        if *p == ".." {
            return Err("outside the set's folder".into());
        }
        d = wait(call2(&d, "getDirectoryHandle", &JsValue::from_str(p), &create)).await.map_err(|e| msg(&e))?;
    }
    let fh = wait(call2(&d, "getFileHandle", &JsValue::from_str(name), &create)).await.map_err(|e| msg(&e))?;
    let w = wait(call0(&fh, "createWritable")).await.map_err(|e| msg(&e))?;
    wait(call1(&w, "write", &Uint8Array::from(data.as_slice()).into())).await.map_err(|e| msg(&e))?;
    wait(call0(&w, "close")).await.map_err(|e| msg(&e))?;
    Ok(())
}

/// Open the browser's folder picker; the folder (or None: cancelled, or why it failed) lands in `slot`.
pub fn pick(slot: FolderSlot, ctx: egui::Context) {
    let Some(win) = web_sys::window() else { return };
    let w: JsValue = win.clone().into();
    if get(&w, "showDirectoryPicker").is_function() {
        let opts = js_sys::Object::new();
        let _ = Reflect::set(&opts, &JsValue::from_str("id"), &JsValue::from_str("cadcraft-sheetset"));
        let _ = Reflect::set(&opts, &JsValue::from_str("mode"), &JsValue::from_str("readwrite"));
        let p = call1(&w, "showDirectoryPicker", &opts.into());
        wasm_bindgen_futures::spawn_local(async move {
            let r = match wait(p).await {
                Err(e) if get(&e, "name").as_string().as_deref() == Some("AbortError") => Ok(None),
                Err(e) => Err(msg(&e)),
                Ok(dir) => {
                    let mut files = Vec::new();
                    let mut total = 0.0;
                    match read_dir(&dir, "", 0, &mut files, &mut total).await {
                        Ok(()) => {
                            files.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
                            let name = get(&dir, "name").as_string().unwrap_or_else(|| "folder".into());
                            let d2 = dir.clone();
                            let write: FolderWrite = Box::new(move |path: &str, data: &[u8]| {
                                let (d, path, data) = (d2.clone(), path.to_string(), data.to_vec());
                                wasm_bindgen_futures::spawn_local(async move {
                                    if let Err(e) = write_file(d, path.clone(), data).await {
                                        web_sys::console::warn_1(&format!("{path}: {e}").into());
                                    }
                                });
                                Ok(())
                            });
                            Ok(Some(PickedFolder { name, files, write: Some(write) }))
                        }
                        Err(e) => Err(e),
                    }
                }
            };
            *slot.borrow_mut() = Some(r);
            ctx.request_repaint();
        });
        return;
    }
    // the fallback: <input type=file webkitdirectory>, read only
    let Some(doc) = win.document() else { return };
    let Ok(inp) = doc.create_element("input") else { return };
    let _ = inp.set_attribute("type", "file");
    let _ = inp.set_attribute("webkitdirectory", "");
    let _ = inp.set_attribute("style", "display:none");
    if let Some(b) = doc.body() {
        let _ = b.append_child(&inp);
    }
    let inp_v: JsValue = inp.clone().into();
    let slot2 = Rc::clone(&slot);
    let ctx2 = ctx.clone();
    let on_change = Closure::once(move || {
        let files = get(&inp_v, "files");
        if let Some(el) = inp_v.dyn_ref::<web_sys::Element>() {
            el.remove();
        }
        wasm_bindgen_futures::spawn_local(async move {
            let n = get(&files, "length").as_f64().unwrap_or(0.0) as u32;
            let r = if n == 0 {
                Ok(None)
            } else {
                let mut out = Vec::new();
                let mut root = String::new();
                let mut total = 0.0;
                let mut err = None;
                for i in 0..n {
                    let f = Reflect::get_u32(&files, i).unwrap_or(JsValue::UNDEFINED);
                    let rel = get(&f, "webkitRelativePath")
                        .as_string()
                        .filter(|s| !s.is_empty())
                        .or_else(|| get(&f, "name").as_string())
                        .unwrap_or_default()
                        .replace('\\', "/");
                    let k = rel.find('/');
                    if root.is_empty()
                        && let Some(k) = k.filter(|k| *k > 0)
                    {
                        root = rel.get(..k).unwrap_or("").to_string();
                    }
                    let key = match k {
                        Some(k) if k > 0 => rel.get(k + 1..).unwrap_or("").to_string(),
                        _ => rel.clone(),
                    };
                    total += get(&f, "size").as_f64().unwrap_or(0.0);
                    if total > MAX_BYTES || out.len() >= MAX_FILES {
                        err = Some("the folder is too big".to_string());
                        break;
                    }
                    match bytes_of(&f).await {
                        Ok(b) => match out.iter_mut().find(|x: &&mut (String, Vec<u8>)| x.0 == key) {
                            Some(x) => x.1 = b,
                            None => out.push((key, b)),
                        },
                        Err(e) => {
                            err = Some(msg(&e));
                            break;
                        }
                    }
                }
                match err {
                    Some(e) => Err(e),
                    None => {
                        out.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
                        Ok(Some(PickedFolder { name: if root.is_empty() { "folder".into() } else { root }, files: out, write: None }))
                    }
                }
            };
            *slot2.borrow_mut() = Some(r);
            ctx2.request_repaint();
        });
    });
    let _ = inp.add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref());
    on_change.forget();
    let slot3 = Rc::clone(&slot);
    let inp_c: JsValue = inp.clone().into();
    let on_cancel = Closure::once(move || {
        if let Some(el) = inp_c.dyn_ref::<web_sys::Element>() {
            el.remove();
        }
        *slot3.borrow_mut() = Some(Ok(None));
        ctx.request_repaint();
    });
    let _ = inp.add_event_listener_with_callback("cancel", on_cancel.as_ref().unchecked_ref());
    on_cancel.forget();
    if let Some(h) = inp.dyn_ref::<web_sys::HtmlElement>() {
        h.click();
    }
}
