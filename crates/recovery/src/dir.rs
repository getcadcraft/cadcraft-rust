//! The desktop's recovery store: a folder in the platform config dir (CLAUDE.md § 6 Recovery), created `0700` on Unix.
//!
//!   <root>/kv/<hex of the key>          one file per key, written atomically (temporary file, fsync, rename)
//!   <root>/sessions/<session>.lock      held (an OS file lock) while that window runs
//!
//! A session whose lock file can be locked by someone else is not running any more (its window quit or crashed): its
//! copies are offered back. Keys and session ids never become paths as they are (hex, or checked to be plain).

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::{MAX_DATA_BYTES, MAX_KEYS, RecoveryOp, Service};

/// The folder store (see the module docs).
#[derive(Debug)]
pub struct DirKv {
    root: PathBuf,
    lock: Option<(String, File)>,
}

fn mkdir(p: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new().recursive(true).mode(0o700).create(p)?;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700))
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(p)
    }
}

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) || s.is_empty() {
        return None;
    }
    let bytes: Option<Vec<u8>> =
        s.as_bytes().chunks(2).map(|h| std::str::from_utf8(h).ok().and_then(|h| u8::from_str_radix(h, 16).ok())).collect();
    String::from_utf8(bytes?).ok()
}

/// A session id usable as a file name.
fn plain(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl DirKv {
    /// The store in `root` (made when missing).
    pub fn open(root: &Path) -> Result<DirKv, String> {
        for d in [root.to_path_buf(), root.join("kv"), root.join("sessions")] {
            mkdir(&d).map_err(|e| format!("the recovery folder {}: {e}", d.display()))?;
        }
        Ok(DirKv { root: root.to_path_buf(), lock: None })
    }

    /// As a [`Service`] for `Services::recovery`.
    pub fn into_service(mut self) -> Service {
        Box::new(move |op| self.op(op))
    }

    fn file(&self, key: &str) -> Result<PathBuf, String> {
        if key.is_empty() || key.len() > 1024 || key.contains('\n') {
            return Err("bad recovery key".into());
        }
        Ok(self.root.join("kv").join(hex(key)))
    }

    pub fn op(&mut self, op: RecoveryOp) -> Result<Option<Vec<u8>>, String> {
        match op {
            RecoveryOp::Ready => Ok(Some(Vec::new())),
            RecoveryOp::Start(s) => {
                if !plain(&s) {
                    return Err("bad session id".into());
                }
                if self.lock.as_ref().is_some_and(|(x, _)| *x == s) {
                    return Ok(None);
                }
                let p = self.root.join("sessions").join(format!("{s}.lock"));
                let f =
                    OpenOptions::new().create(true).truncate(false).write(true).open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                f.try_lock().map_err(|e| format!("{}: {e}", p.display()))?;
                self.lock = Some((s, f));
                Ok(None)
            }
            RecoveryOp::Stop(s) => {
                if let Some((x, f)) = self.lock.take() {
                    if x == s {
                        let _ = f.unlock();
                        drop(f);
                        let _ = fs::remove_file(self.root.join("sessions").join(format!("{s}.lock")));
                    } else {
                        self.lock = Some((x, f));
                    }
                }
                Ok(None)
            }
            RecoveryOp::Put(k, v) => {
                let p = self.file(&k)?;
                write_atomic(&p, &v).map_err(|e| format!("{}: {e}", p.display()))?;
                Ok(None)
            }
            RecoveryOp::Get(k) => {
                let p = self.file(&k)?;
                match File::open(&p) {
                    Ok(f) => {
                        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
                        if usize::try_from(len).map_or(true, |l| l > MAX_DATA_BYTES) {
                            return Err(format!("{}: too big to be a recovery copy", p.display()));
                        }
                        let mut out = Vec::new();
                        f.take(u64::try_from(MAX_DATA_BYTES).unwrap_or(u64::MAX))
                            .read_to_end(&mut out)
                            .map_err(|e| format!("{}: {e}", p.display()))?;
                        Ok(Some(out))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(e) => Err(format!("{}: {e}", p.display())),
                }
            }
            RecoveryOp::Delete(k) => {
                let p = self.file(&k)?;
                match fs::remove_file(&p) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", p.display())),
                    _ => Ok(None),
                }
            }
            RecoveryOp::List => {
                let rd = fs::read_dir(self.root.join("kv")).map_err(|e| e.to_string())?;
                let mut keys: Vec<String> = rd
                    .filter_map(Result::ok)
                    .take(MAX_KEYS)
                    .filter_map(|e| e.file_name().to_str().and_then(unhex))
                    .filter(|k| !k.contains('\n'))
                    .collect();
                keys.sort();
                Ok(Some(keys.join("\n").into_bytes()))
            }
            RecoveryOp::Live => {
                let mine = self.lock.as_ref().map(|(s, _)| s.clone());
                let mut live = Vec::new();
                let dir = self.root.join("sessions");
                let rd = fs::read_dir(&dir).map_err(|e| e.to_string())?;
                for e in rd.filter_map(Result::ok).take(MAX_KEYS) {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let Some(s) = name.strip_suffix(".lock").filter(|s| plain(s)) else { continue };
                    if mine.as_deref() == Some(s) {
                        live.push(s.to_string());
                        continue;
                    }
                    let Ok(f) = OpenOptions::new().write(true).open(e.path()) else { continue };
                    match f.try_lock() {
                        // nobody holds it: that window is gone
                        Ok(()) => {
                            let _ = f.unlock();
                            drop(f);
                            let _ = fs::remove_file(e.path());
                        }
                        Err(_) => live.push(s.to_string()),
                    }
                }
                live.sort();
                Ok(Some(live.join("\n").into_bytes()))
            }
        }
    }
}

/// Stage in the same folder, fsync, rename over the target (the target stays whole if this stops half way).
fn write_atomic(p: &Path, data: &[u8]) -> std::io::Result<()> {
    let tmp = p.with_extension("tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, p)?;
    #[cfg(unix)]
    if let Some(d) = p.parent()
        && let Ok(df) = File::open(d)
    {
        let _ = df.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Autosave;
    use serde_json::json;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc-recovery-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn crash_then_offer_through_the_folder() {
        let root = tmp("crash");
        let mut a = Autosave::new("cadcraft", "aaaa", DirKv::open(&root).unwrap().into_service());
        assert!(a.start(0.0));
        a.doc("tab1", "Plan");
        a.changed("tab1", 0.0);
        assert!(a.save("tab1", Some((b"DWG".to_vec(), json!({"name": "Plan"}))), 2000.0));
        // a second window while the first runs: nothing to offer
        let mut b = Autosave::new("cadcraft", "bbbb", DirKv::open(&root).unwrap().into_service());
        assert!(b.start(3000.0));
        assert!(b.orphans(3000.0).is_empty());
        // the first one "crashes" (its lock goes with the process; here: dropped)
        drop(a);
        let o = b.orphans(4000.0);
        assert_eq!(o.len(), 1);
        let (r, d) = b.load(&o[0].key).unwrap();
        assert_eq!((r.name.as_str(), d.as_slice()), ("Plan", b"DWG".as_slice()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&root).unwrap().permissions().mode() & 0o777, 0o700);
        }
        b.stop();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hostile_files_are_refused() {
        let root = tmp("hostile");
        let mut kv = DirKv::open(&root).unwrap();
        assert!(kv.op(RecoveryOp::Start("../x".into())).is_err());
        assert!(kv.op(RecoveryOp::Put(String::new(), vec![])).is_err());
        // stray files in the folder are not keys
        fs::write(root.join("kv").join("zz"), b"x").unwrap();
        fs::write(root.join("kv").join("6"), b"x").unwrap();
        assert_eq!(kv.op(RecoveryOp::List).unwrap(), Some(Vec::new()));
        // a damaged index record
        fs::write(root.join("kv").join(hex("i:cadcraft:x:t")), b"\xff\xfe garbage").unwrap();
        let mut a = Autosave::new("cadcraft", "cccc", kv.into_service());
        a.start(0.0);
        assert!(a.orphans(1.0).is_empty());
        assert_eq!(a.damaged, vec!["i:cadcraft:x:t".to_string()]);
        let _ = fs::remove_dir_all(&root);
    }
}
