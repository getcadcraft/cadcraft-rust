//! Recovery copies in the browser (CLAUDE.md § 6 Recovery): IndexedDB "cadcraft-rust-recovery", one object store
//! "kv" of byte arrays by key. The store is read once at start into a mirror (the window's recovery code is
//! synchronous: `Services::recovery`); reads come from the mirror, writes and deletes go to both, and a write the
//! browser refuses (storage full, private window) is reported on the next call. Heartbeats, not locks, tell which
//! tabs still run (as 0.5 without Web Locks).

use std::cell::RefCell;
use std::rc::Rc;

use cc_recovery::{RecoveryOp, Service};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

const DB: &str = "cadcraft-rust-recovery";
const STORE: &str = "kv";

#[derive(Default)]
struct Mirror {
    ready: bool,
    /// the database could not be opened: why
    failed: Option<String>,
    db: Option<web_sys::IdbDatabase>,
    items: Vec<(String, Vec<u8>)>,
    /// a write the browser refused since the last call
    error: Option<String>,
}

fn js_err(e: &JsValue) -> String {
    if let Some(d) = e.dyn_ref::<web_sys::DomException>() {
        return format!("{}: {}", d.name(), d.message());
    }
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

/// A request's result as a future.
async fn done(req: &web_sys::IdbRequest) -> Result<JsValue, String> {
    let r2 = req.clone();
    let p = js_sys::Promise::new(&mut |resolve, reject| {
        let ok = r2.clone();
        let on_ok = Closure::once_into_js(move || {
            let _ = resolve.call1(&JsValue::NULL, &ok.result().unwrap_or(JsValue::UNDEFINED));
        });
        let bad = r2.clone();
        let on_err = Closure::once_into_js(move || {
            let e = bad.error().ok().flatten().map_or(JsValue::from_str("IndexedDB error"), JsValue::from);
            let _ = reject.call1(&JsValue::NULL, &e);
        });
        r2.set_onsuccess(Some(on_ok.unchecked_ref()));
        r2.set_onerror(Some(on_err.unchecked_ref()));
    });
    wasm_bindgen_futures::JsFuture::from(p).await.map_err(|e| js_err(&e))
}

async fn load(m: Rc<RefCell<Mirror>>) -> Result<(), String> {
    let f = web_sys::window().and_then(|w| w.indexed_db().ok().flatten()).ok_or("IndexedDB is not available here.")?;
    let open = f.open_with_u32(DB, 1).map_err(|e| js_err(&e))?;
    let up = open.clone();
    let on_up = Closure::once_into_js(move || {
        if let Ok(r) = up.result()
            && let Ok(db) = r.dyn_into::<web_sys::IdbDatabase>()
            && !db.object_store_names().contains(STORE)
        {
            let _ = db.create_object_store(STORE);
        }
    });
    open.set_onupgradeneeded(Some(on_up.unchecked_ref()));
    let db: web_sys::IdbDatabase = done(&open).await?.dyn_into().map_err(|_| "IndexedDB: no database".to_string())?;
    let tx = db.transaction_with_str(STORE).map_err(|e| js_err(&e))?;
    let st = tx.object_store(STORE).map_err(|e| js_err(&e))?;
    let keys = done(&st.get_all_keys().map_err(|e| js_err(&e))?).await?;
    let vals = done(&st.get_all().map_err(|e| js_err(&e))?).await?;
    let keys: js_sys::Array = keys.dyn_into().map_err(|_| "IndexedDB: keys".to_string())?;
    let vals: js_sys::Array = vals.dyn_into().map_err(|_| "IndexedDB: values".to_string())?;
    let mut items = Vec::new();
    for (k, v) in keys.iter().zip(vals.iter()) {
        let (Some(k), Ok(v)) = (k.as_string(), v.dyn_into::<js_sys::Uint8Array>()) else { continue };
        if v.length() as usize > cc_recovery::MAX_DATA_BYTES {
            continue;
        }
        items.push((k, v.to_vec()));
    }
    let mut s = m.borrow_mut();
    s.items = items;
    s.db = Some(db);
    s.ready = true;
    Ok(())
}

/// A write or delete in the database; a refusal is kept for the next call.
fn write(m: &Rc<RefCell<Mirror>>, key: &str, val: Option<&[u8]>) {
    let Some(db) = m.borrow().db.clone() else { return };
    let r = (|| -> Result<(), JsValue> {
        let tx = db.transaction_with_str_and_mode(STORE, web_sys::IdbTransactionMode::Readwrite)?;
        let st = tx.object_store(STORE)?;
        match val {
            Some(v) => {
                st.put_with_key(&js_sys::Uint8Array::from(v), &JsValue::from_str(key))?;
            }
            None => {
                st.delete(&JsValue::from_str(key))?;
            }
        }
        let m2 = Rc::clone(m);
        let t2 = tx.clone();
        let on_fail = Closure::once_into_js(move || {
            let e = t2.error().map_or_else(|| "IndexedDB write failed".to_string(), |d| format!("{}: {}", d.name(), d.message()));
            m2.borrow_mut().error = Some(e);
        });
        tx.set_onabort(Some(on_fail.unchecked_ref()));
        Ok(())
    })();
    if let Err(e) = r {
        m.borrow_mut().error = Some(js_err(&e));
    }
}

/// The service for `Services::recovery`.
pub fn service() -> Service {
    let m: Rc<RefCell<Mirror>> = Rc::default();
    let m2 = Rc::clone(&m);
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = load(Rc::clone(&m2)).await {
            m2.borrow_mut().failed = Some(e);
        }
    });
    Box::new(move |op| {
        if let Some(e) = m.borrow().failed.clone() {
            return Err(e);
        }
        if matches!(op, RecoveryOp::Put(..) | RecoveryOp::Delete(_)) {
            let refused = m.borrow_mut().error.take();
            if let Some(e) = refused {
                return Err(e);
            }
        }
        match op {
            RecoveryOp::Ready => Ok(m.borrow().ready.then(Vec::new)),
            RecoveryOp::Start(_) | RecoveryOp::Stop(_) | RecoveryOp::Live => Ok(None),
            RecoveryOp::Put(k, v) => {
                write(&m, &k, Some(&v));
                let mut s = m.borrow_mut();
                match s.items.iter_mut().find(|(kk, _)| *kk == k) {
                    Some(slot) => slot.1 = v,
                    None => {
                        let at = s.items.partition_point(|(kk, _)| *kk < k);
                        s.items.insert(at, (k, v));
                    }
                }
                Ok(None)
            }
            RecoveryOp::Get(k) => Ok(m.borrow().items.iter().find(|(kk, _)| *kk == k).map(|(_, v)| v.clone())),
            RecoveryOp::Delete(k) => {
                write(&m, &k, None);
                m.borrow_mut().items.retain(|(kk, _)| *kk != k);
                Ok(None)
            }
            RecoveryOp::List => Ok(Some(m.borrow().items.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join("\n").into_bytes())),
        }
    })
}
