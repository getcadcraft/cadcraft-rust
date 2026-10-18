//! Files only inside the folders the person names — tools/cli/headless/paths.mjs (0.5) with AGENTS.md § 9's extra
//! rules: separate read and write roots (`--automation-read-root` / `--automation-write-root`; 0.5's `--root` adds a
//! folder to both), confinement on by default (the current folder when nothing is named), every path canonicalised
//! (symlinks followed, `..` gone) and checked against each root's real path; device paths (`\\?\`, `\\.\`), UNC paths,
//! NUL bytes and, on Windows, alternate data streams and reserved device names are refused before the file system is
//! asked anything. A new file's folder must already exist and be inside a write root; a file that exists is replaced
//! only when this session opened or wrote it, or with `overwrite`; the file itself may not be a link.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Why a path was refused (the message is what the assistant reads).
#[derive(Debug, Clone, PartialEq)]
pub struct PathError {
    pub message: String,
    /// outside | missing | invalid | noroots | exists
    pub code: &'static str,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

fn err(message: impl Into<String>, code: &'static str) -> PathError {
    PathError { message: message.into(), code }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

fn expand(p: &str) -> PathBuf {
    if p == "~" {
        return home().unwrap_or_else(|| PathBuf::from(p));
    }
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\"))
        && let Some(h) = home()
    {
        return h.join(rest);
    }
    PathBuf::from(p)
}

/// `path.resolve`: absolute, `.` and `..` folded lexically (no file system access).
fn resolve_lexical(base: &Path, p: &Path) -> PathBuf {
    let joined = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn inside(child: &Path, parent: &Path) -> bool {
    child.starts_with(parent)
}

fn shown(list: &[PathBuf]) -> String {
    list.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
}

/// Refuse what no root can make safe, whatever the platform (paths.mjs `abs`, AGENTS § 9).
fn refuse_form(p: &str) -> Result<(), PathError> {
    if p.trim().is_empty() {
        return Err(err("A file path is needed.", "invalid"));
    }
    if p.contains('\0') {
        return Err(err("That path is not allowed.", "invalid"));
    }
    if p.starts_with("\\\\?\\") || p.starts_with("\\\\.\\") || p.starts_with("//?/") || p.starts_with("//./") {
        return Err(err("Device paths are not allowed.", "invalid"));
    }
    #[cfg(windows)]
    {
        if p.starts_with("\\\\") || p.starts_with("//") {
            return Err(err("Network (UNC) paths are not allowed.", "invalid"));
        }
        // a drive letter's colon is the only one allowed: anything else names an alternate data stream
        let tail = if p.len() >= 2 && p.as_bytes().get(1) == Some(&b':') { p.get(2..).unwrap_or("") } else { p };
        if tail.contains(':') {
            return Err(err("That path is not allowed.", "invalid"));
        }
        const RESERVED: [&str; 22] = [
            "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3",
            "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
        ];
        for part in p.split(['\\', '/']) {
            let stem = part.split('.').next().unwrap_or("").trim_end().to_ascii_lowercase();
            if RESERVED.contains(&stem.as_str()) {
                return Err(err("Device names are not allowed.", "invalid"));
            }
        }
    }
    Ok(())
}

/// One list of folders (read or write), as named and as their real paths.
#[derive(Debug, Clone, Default)]
struct RootList {
    real: Vec<PathBuf>,
    raw: Vec<PathBuf>,
}

impl RootList {
    fn add(&mut self, folder: &str) -> Result<(), PathError> {
        let cwd = std::env::current_dir().map_err(|e| err(format!("The current folder cannot be read: {e}"), "missing"))?;
        let abs = resolve_lexical(&cwd, &expand(folder));
        let real = std::fs::canonicalize(&abs).map_err(|_| err(format!("The folder {} does not exist.", abs.display()), "missing"))?;
        if !real.is_dir() {
            return Err(err(format!("{} is not a folder.", abs.display()), "missing"));
        }
        if !self.real.contains(&real) {
            self.real.push(real);
        }
        if !self.raw.contains(&abs) {
            self.raw.push(abs);
        }
        Ok(())
    }
    fn root_of(&self, real: &Path) -> Option<&PathBuf> {
        self.real.iter().find(|r| inside(real, r))
    }
}

/// The folders an assistant may read and write.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    read: RootList,
    write: RootList,
}

impl Roots {
    /// `both` = 0.5's `--root` (read and write); `read_only` / `write_only` = `--automation-read-root` /
    /// `--automation-write-root`. Nothing named = the current folder for both (AGENTS § 9).
    pub fn new(both: &[String], read_only: &[String], write_only: &[String]) -> Result<Roots, PathError> {
        let mut r = Roots::default();
        if both.is_empty() && read_only.is_empty() && write_only.is_empty() {
            r.read.add(".")?;
            r.write.add(".")?;
            return Ok(r);
        }
        for f in both {
            r.read.add(f)?;
            r.write.add(f)?;
        }
        for f in read_only {
            r.read.add(f)?;
        }
        for f in write_only {
            r.write.add(f)?;
            // a folder you may write you may also read back
            r.read.add(f)?;
        }
        Ok(r)
    }

    /// No folders at all (refuses every path).
    pub fn none() -> Roots {
        Roots::default()
    }

    pub fn any(&self) -> bool {
        !self.read.real.is_empty() || !self.write.real.is_empty()
    }
    /// The readable folders (real paths), for messages and `files_list`.
    pub fn list(&self) -> &[PathBuf] {
        &self.read.real
    }
    pub fn write_list(&self) -> &[PathBuf] {
        &self.write.real
    }
    /// The root holding a real path (readable roots).
    pub fn root_of(&self, real: &Path) -> Option<&PathBuf> {
        self.read.root_of(real)
    }

    fn abs(&self, p: &str, list: &RootList) -> Result<PathBuf, PathError> {
        refuse_form(p)?;
        let Some(first) = list.real.first() else {
            return Err(err("No folders are open to the assistant. Start it with --root <folder>.", "noroots"));
        };
        Ok(resolve_lexical(first, &expand(p)))
    }

    fn lexical(a: &Path, list: &RootList) -> Result<(), PathError> {
        if !list.real.iter().chain(list.raw.iter()).any(|r| inside(a, r)) {
            return Err(err(format!("{} is outside the folders the assistant may use ({}).", a.display(), shown(&list.real)), "outside"));
        }
        Ok(())
    }

    fn check(real: &Path, a: &Path, list: &RootList) -> Result<(), PathError> {
        if list.root_of(real).is_none() {
            return Err(err(format!("{} is outside the folders the assistant may use ({}).", a.display(), shown(&list.real)), "outside"));
        }
        Ok(())
    }

    /// An existing file to read → its real path.
    pub fn read(&self, p: &str) -> Result<PathBuf, PathError> {
        let a = self.abs(p, &self.read)?;
        Self::lexical(&a, &self.read)?;
        let real = std::fs::canonicalize(&a).map_err(|_| err(format!("There is no file {}.", a.display()), "missing"))?;
        Self::check(&real, &a, &self.read)?;
        let md = std::fs::metadata(&real).map_err(|_| err(format!("There is no file {}.", a.display()), "missing"))?;
        if !md.is_file() {
            return Err(err(format!("{} is not a file.", a.display()), "invalid"));
        }
        Ok(real)
    }

    /// A folder to list (None = the first readable folder).
    pub fn folder(&self, p: Option<&str>) -> Result<PathBuf, PathError> {
        let Some(first) = self.read.real.first() else {
            return Err(err("No folders are open to the assistant. Start it with --root <folder>.", "noroots"));
        };
        let a = match p {
            Some(p) => {
                let a = self.abs(p, &self.read)?;
                Self::lexical(&a, &self.read)?;
                a
            }
            None => first.clone(),
        };
        let real = std::fs::canonicalize(&a).map_err(|_| err(format!("There is no folder {}.", a.display()), "missing"))?;
        Self::check(&real, &a, &self.read)?;
        if !real.is_dir() {
            return Err(err(format!("{} is not a folder.", a.display()), "invalid"));
        }
        Ok(real)
    }

    /// A file to write → the real path to write. `mine` = paths this session opened or wrote (they may be replaced).
    pub fn write(&self, p: &str, overwrite: bool, mine: &HashSet<PathBuf>) -> Result<PathBuf, PathError> {
        let a = self.abs(p, &self.write)?;
        Self::lexical(&a, &self.write)?;
        let dir = a.parent().map(Path::to_path_buf).unwrap_or_default();
        let name = a.file_name().ok_or_else(|| err("A file name is needed.", "invalid"))?;
        let real_dir = std::fs::canonicalize(&dir).map_err(|_| err(format!("The folder {} does not exist.", dir.display()), "missing"))?;
        let real = real_dir.join(name);
        Self::check(&real, &a, &self.write)?;
        if let Ok(st) = std::fs::symlink_metadata(&real) {
            if st.file_type().is_symlink() {
                return Err(err(format!("{} is a link; write to the file it points to instead.", a.display()), "invalid"));
            }
            if !st.is_file() {
                return Err(err(format!("{} is not a file.", a.display()), "invalid"));
            }
            if !overwrite && !mine.contains(&real) {
                return Err(err(format!("{} already exists. Pass overwrite: true to replace it.", a.display()), "exists"));
            }
        }
        Ok(real)
    }
}

/// Write atomically: a temp file beside it (synced), then rename (paths.mjs `writeFileAtomic`, AGENTS § 6).
pub fn write_file_atomic(real: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let dir = real.parent().ok_or("no folder")?;
    let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stamp = crate::rpc::now_ms();
    let tmp = dir.join(format!(".{name}.{}.{stamp}.tmp", std::process::id()));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, real)?;
        Ok(())
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", real.display()));
    }
    #[cfg(unix)]
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// One file `files_list` reports.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub path: String,
    pub size: u64,
    pub modified: String,
}

/// Files in a folder (inside the roots), newest first; ≤ max (paths.mjs `listFiles`). Hidden files are skipped and
/// links are not followed. → (folder, files, truncated).
pub fn list_files(
    roots: &Roots,
    folder: Option<&str>,
    extensions: Option<&[String]>,
    recursive: bool,
    max: usize,
) -> Result<(PathBuf, Vec<Listed>, bool), PathError> {
    let base = roots.folder(folder)?;
    let exts: Option<HashSet<String>> =
        extensions.filter(|e| !e.is_empty()).map(|e| e.iter().map(|x| format!(".{}", x.trim_start_matches('.').to_lowercase())).collect());
    let mut out: Vec<Listed> = Vec::new();
    let mut truncated = false;
    // an explicit stack, depth ≤ 12 (folder trees can be deep and cyclic links are not followed anyway)
    fn walk(
        dir: &Path,
        depth: usize,
        recursive: bool,
        exts: &Option<HashSet<String>>,
        max: usize,
        out: &mut Vec<Listed>,
        truncated: &mut bool,
    ) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for ent in rd.flatten() {
            if out.len() >= max {
                *truncated = true;
                return;
            }
            let name = ent.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(ft) = ent.file_type() else { continue };
            let p = ent.path();
            if ft.is_dir() {
                if recursive && depth < 12 {
                    walk(&p, depth.saturating_add(1), recursive, exts, max, out, truncated);
                }
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            if let Some(ex) = exts {
                let e = p.extension().map(|x| format!(".{}", x.to_string_lossy().to_lowercase())).unwrap_or_default();
                if !ex.contains(&e) {
                    continue;
                }
            }
            if let Ok(md) = std::fs::metadata(&p) {
                let ms =
                    md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_millis() as f64);
                out.push(Listed { path: p.display().to_string(), size: md.len(), modified: crate::rpc::iso_time(ms) });
            }
        }
    }
    walk(&base, 0, recursive, &exts, max, &mut out, &mut truncated);
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok((base, out, truncated))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc-mcp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    #[test]
    fn confinement() {
        let base = tmpdir("confine");
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("a.dwg"), b"x").unwrap();
        std::fs::write(outside.join("secret.txt"), b"s").unwrap();
        let r = Roots::new(&[root.display().to_string()], &[], &[]).unwrap();
        assert_eq!(r.read("a.dwg").unwrap(), root.join("a.dwg"));
        assert_eq!(r.read(&root.join("sub/../a.dwg").display().to_string()).unwrap(), root.join("a.dwg"));
        assert_eq!(r.read("../outside/secret.txt").unwrap_err().code, "outside");
        assert_eq!(r.read(&outside.join("secret.txt").display().to_string()).unwrap_err().code, "outside");
        assert_eq!(r.read("missing.dwg").unwrap_err().code, "missing");
        assert_eq!(r.read("sub").unwrap_err().code, "invalid");
        assert_eq!(r.read("").unwrap_err().code, "invalid");
        assert_eq!(r.read("a\0b").unwrap_err().code, "invalid");
        assert_eq!(r.read("\\\\?\\C:\\x").unwrap_err().message, "Device paths are not allowed.");
        #[cfg(unix)]
        {
            // a link inside that points out is refused after canonicalising
            std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();
            assert_eq!(r.read("link.txt").unwrap_err().code, "outside");
            std::os::unix::fs::symlink(&outside, root.join("dirlink")).unwrap();
            assert_eq!(r.read("dirlink/secret.txt").unwrap_err().code, "outside");
            assert_eq!(r.write("dirlink/new.txt", false, &HashSet::new()).unwrap_err().code, "outside");
            // writing onto a link is refused even inside
            std::fs::write(root.join("t.txt"), b"t").unwrap();
            std::os::unix::fs::symlink(root.join("t.txt"), root.join("tlink.txt")).unwrap();
            assert_eq!(r.write("tlink.txt", true, &HashSet::new()).unwrap_err().code, "invalid");
        }
        let mine = HashSet::new();
        assert_eq!(r.write("new.pdf", false, &mine).unwrap(), root.join("new.pdf"));
        assert_eq!(r.write("a.dwg", false, &mine).unwrap_err().code, "exists");
        assert!(r.write("a.dwg", true, &mine).is_ok());
        let mine: HashSet<PathBuf> = [root.join("a.dwg")].into_iter().collect();
        assert!(r.write("a.dwg", false, &mine).is_ok());
        assert_eq!(r.write("nofolder/x.pdf", false, &mine).unwrap_err().code, "missing");
        assert_eq!(r.write("../outside/x.pdf", false, &mine).unwrap_err().code, "outside");
        // read-only roots refuse writes
        let ro = Roots::new(&[], &[root.display().to_string()], &[outside.display().to_string()]).unwrap();
        assert!(ro.read("a.dwg").is_ok());
        assert_eq!(ro.write(&root.join("w.txt").display().to_string(), false, &HashSet::new()).unwrap_err().code, "outside");
        assert!(ro.write(&outside.join("w.txt").display().to_string(), false, &HashSet::new()).is_ok());
        // no roots: nothing
        assert_eq!(Roots::none().read("a.dwg").unwrap_err().code, "noroots");
        // listing
        let (folder, files, truncated) = list_files(&r, None, Some(&["DWG".to_string()]), false, 2000).unwrap();
        assert_eq!(folder, root);
        assert!(!truncated);
        assert_eq!(files.len(), 1);
        write_file_atomic(&root.join("w.bin"), b"abc").unwrap();
        assert_eq!(std::fs::read(root.join("w.bin")).unwrap(), b"abc");
        let _ = std::fs::remove_dir_all(&base);
    }
}
