//! A project folder for the DWG view (0.5: the folder opened in BimCraft — Studio's folder picker — walked by
//! folder-pick.js, indexed by folder-scan.js `indexFiles`), and bimcraft.js's lookups in it: `findLinked` (a linked
//! drawing by "<name>.dwg", the drawing's own folder first) and `findPicture` (the same name, its folder first; else a
//! file whose name holds the picture's name with the same extension, as eTransmit and Revit exports prefix the
//! project name: "…-studio_logo-5425051.png" for "studio_logo.png" — "Shown, similar name").
//!
//! Paths are relative to the folder with "/" separators. Files added by hand are looked up first.

use std::collections::HashMap;

use crate::tr;

/// folder-pick.js `MAX_FILES`.
pub const MAX_FILES: usize = 20_000;

/// folder-scan.js `HOUSEKEEPING_DIR`: Revit's own folders and hidden ones, noted, not walked.
pub fn housekeeping(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with("_backup") || l.ends_with("_log") || l == "revit_temp" || l.starts_with('.') || l == "__macosx"
}

/// A walked folder: its name, its files (relative paths, in the order listed), the housekeeping folders skipped and
/// the files by lower-case name.
#[derive(Clone, Debug, Default)]
pub struct Folder {
    pub name: String,
    pub files: Vec<String>,
    pub skipped: Vec<String>,
    by_name: HashMap<String, Vec<usize>>,
    names: Vec<String>,
}

impl Folder {
    /// `indexFiles(files)` over relative paths.
    pub fn new(name: &str, files: Vec<String>, skipped: Vec<String>) -> Folder {
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        let mut names: Vec<String> = Vec::new();
        for (i, p) in files.iter().enumerate() {
            let n = base_of(p).to_lowercase();
            let slot = by_name.entry(n.clone()).or_default();
            if slot.is_empty() {
                names.push(n);
            }
            slot.push(i);
        }
        Folder { name: name.to_string(), files, skipped, by_name, names }
    }
    fn hits(&self, lower_name: &str) -> Vec<&str> {
        self.by_name.get(lower_name).map(|v| v.iter().filter_map(|&i| self.files.get(i).map(String::as_str)).collect()).unwrap_or_default()
    }
}

/// `baseOf(path)`.
pub fn base_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
/// `path.slice(0, path.lastIndexOf('/') + 1)`.
fn dir_slash(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => path.get(..=i).unwrap_or(""),
        None => "",
    }
}

/// Walk a folder as folder-pick.js `fromHandle` does: `list(relative dir)` gives (name, is folder) in its own order;
/// housekeeping folders are noted, not walked; more than [`MAX_FILES`] files is an error (`folder.toomany`).
/// A folder lister: relative path → (name, is a folder).
pub type Lister<'a> = dyn FnMut(&str) -> Result<Vec<(String, bool)>, String> + 'a;

pub fn walk(name: &str, list: &mut Lister<'_>) -> Result<Folder, String> {
    let mut files: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    // depth-first, in listing order (an explicit stack: deep trees cannot overflow ours)
    let mut stack: Vec<(String, std::vec::IntoIter<(String, bool)>)> = vec![(String::new(), list("")?.into_iter())];
    let mut depth_guard = 0usize;
    while let Some((prefix, mut it)) = stack.pop() {
        let Some((n, dir)) = it.next() else { continue };
        let path = if prefix.is_empty() { n.clone() } else { format!("{prefix}/{n}") };
        stack.push((prefix, it));
        if dir {
            if housekeeping(&n) {
                skipped.push(path);
            } else if stack.len() < 256 {
                depth_guard = depth_guard.saturating_add(1);
                if depth_guard > 200_000 {
                    break;
                }
                let kids = list(&path).unwrap_or_default();
                stack.push((path, kids.into_iter()));
            }
        } else {
            if files.len() >= MAX_FILES {
                return Err(tr("folder.toomany", &[("max", crate::num(MAX_FILES))]));
            }
            files.push(path);
        }
    }
    Ok(Folder::new(name, files, skipped))
}

/// Where a linked file or picture was found.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    /// added by hand: its lower-case name
    Added(String),
    /// in the folder: its relative path
    Path(String),
}

/// A lookup's answer: where, what to show as "where", and whether the name matched exactly.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub found: Found,
    pub where_: String,
    pub exact: bool,
}

/// `findLinked(name, path)`: "<name>.dwg" added by hand, else in the folder (the drawing's own folder first).
pub fn find_linked(name: &str, path: Option<&str>, added: &[String], folder: Option<&Folder>) -> Option<Hit> {
    let want = format!("{name}.dwg").to_lowercase();
    if added.contains(&want) {
        return Some(Hit { found: Found::Added(want), where_: tr("dwg.addedbyyou", &[]), exact: true });
    }
    let folder = folder?;
    let hits = folder.hits(&want);
    // `dir ? dir + '/' : ''` (a path "/x.dwg" has the empty folder)
    let dir = match path {
        Some(p) if p.contains('/') => {
            let d = p.get(..p.rfind('/').unwrap_or(0)).unwrap_or("");
            if d.is_empty() { String::new() } else { format!("{d}/") }
        }
        _ => String::new(),
    };
    let best = hits.iter().find(|h| dir_slash(h) == dir).or(hits.first())?;
    Some(Hit { found: Found::Path((*best).to_string()), where_: (*best).to_string(), exact: true })
}

/// `findPicture(name, path)`.
pub fn find_picture(name: &str, path: Option<&str>, added: &[String], folder: Option<&Folder>) -> Option<Hit> {
    let want = name.to_lowercase();
    if want.is_empty() {
        return None;
    }
    if added.contains(&want) {
        return Some(Hit { found: Found::Added(want), where_: tr("dwg.addedbyyou", &[]), exact: true });
    }
    let dir = path.map_or("", dir_slash);
    let near = |hits: &[&str]| -> Option<String> { hits.iter().find(|h| dir_slash(h) == dir).or(hits.first()).map(|h| (*h).to_string()) };
    if let Some(f) = folder {
        let hs = f.hits(&want);
        if !hs.is_empty() {
            let h = near(&hs)?;
            return Some(Hit { found: Found::Path(h.clone()), where_: h, exact: true });
        }
    }
    let (stem, ext) = match want.rfind('.') {
        Some(i) if i > 0 => (want.get(..i).unwrap_or(""), want.get(i..).unwrap_or("")),
        _ => (want.as_str(), ""),
    };
    if stem.encode_utf16().count() < 4 {
        return None;
    }
    let like = |n: &str| n.ends_with(ext) && n.contains(stem);
    if let Some(n) = added.iter().find(|n| like(n)) {
        return Some(Hit { found: Found::Added(n.clone()), where_: tr("dwg.addedbyyou", &[]), exact: false });
    }
    if let Some(f) = folder {
        let hits: Vec<&str> = f.names.iter().filter(|n| like(n)).flat_map(|n| f.hits(n)).collect();
        if !hits.is_empty() {
            let h = near(&hits)?;
            return Some(Hit { found: Found::Path(h.clone()), where_: h, exact: false });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake() -> HashMap<String, Vec<(String, bool)>> {
        let mut m = HashMap::new();
        m.insert(
            "".into(),
            vec![("Plans".into(), true), ("logo.png".into(), false), ("Project_backup".into(), true), ("X-Refs".into(), true)],
        );
        m.insert("Plans".into(), vec![("A.dwg".into(), false), ("Base.dwg".into(), false)]);
        m.insert("X-Refs".into(), vec![("Base.dwg".into(), false), ("Proj-studio_logo-1.PNG".into(), false)]);
        m
    }

    #[test]
    fn walk_and_find() {
        cc_lang::with(cc_lang::Locale::En, || {
            let m = fake();
            let f = walk("Job", &mut |d| Ok(m.get(d).cloned().unwrap_or_default())).unwrap();
            assert_eq!(f.files, vec!["Plans/A.dwg", "Plans/Base.dwg", "logo.png", "X-Refs/Base.dwg", "X-Refs/Proj-studio_logo-1.PNG"]);
            assert_eq!(f.skipped, vec!["Project_backup"]);
            // the drawing's own folder first, then the first found
            assert_eq!(find_linked("base", Some("Plans/A.dwg"), &[], Some(&f)).unwrap().where_, "Plans/Base.dwg");
            assert_eq!(find_linked("BASE", Some("Other/A.dwg"), &[], Some(&f)).unwrap().where_, "Plans/Base.dwg");
            assert_eq!(find_linked("base", None, &["base.dwg".into()], Some(&f)).unwrap().where_, "added by you");
            assert!(find_linked("nope", Some("Plans/A.dwg"), &[], Some(&f)).is_none());
            // a drawing at "/x.dwg" sits in the folder's top (dwgviewdiff --lookup)
            let top = Folder::new("T", vec!["Sub/base.dwg".into(), "Base.DWG".into()], vec![]);
            assert_eq!(find_linked("base", Some("/x.dwg"), &[], Some(&top)).unwrap().where_, "Base.DWG");
            let p = find_picture("studio_logo.png", Some("Plans/A.dwg"), &[], Some(&f)).unwrap();
            assert_eq!((p.where_.as_str(), p.exact), ("X-Refs/Proj-studio_logo-1.PNG", false));
            assert!(find_picture("logo.png", None, &[], Some(&f)).unwrap().exact);
            assert!(find_picture("abc.png", None, &[], Some(&f)).is_none(), "a stem under 4 characters never matches loosely");
            let mut n = 0usize;
            let too = walk("Big", &mut |d| {
                n += 1;
                Ok(if d.is_empty() { (0..MAX_FILES + 1).map(|i| (format!("f{i}"), false)).collect() } else { vec![] })
            });
            assert!(too.is_err());
        });
    }
}
