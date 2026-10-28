//! DELBLOCKS, delete blocks without a routine (delblocks.js): every reference goes — in the model, on every sheet and
//! nested inside other blocks' definitions — and then the definitions. The whole command is one undo step.

use cc_cad::Doc;
use cc_cad::entity::{Entity, Geo};

use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Opt;
use crate::prompt::Kind;

/// ICU's root order for ASCII names, roughly: case and accents ignored at first (sensitivity "base").
fn base_key(s: &str) -> Vec<u32> {
    s.chars().flat_map(char::to_lowercase).map(u32::from).collect()
}

/// Block names DELBLOCKS offers, sorted.
pub fn deletable(doc: &Doc) -> Vec<String> {
    let mut v: Vec<String> = doc
        .blocks
        .values()
        .filter(|b| {
            !b.name.starts_with('*')
                && b.xref.is_none()
                && !{
                    let l = b.name.to_lowercase();
                    l.starts_with("*model_space") || l.starts_with("*paper_space")
                }
        })
        .map(|b| b.name.clone())
        .collect();
    v.sort_by_key(|n| base_key(n));
    v
}

fn block_of(e: &Entity) -> Option<&str> {
    match &e.geo {
        Geo::Insert(i) => Some(i.block.as_str()),
        _ => None,
    }
}

/// What DELBLOCKS did: (block, references erased) and (block, why it stayed).
type Deleted = (Vec<(String, usize)>, Vec<(String, String)>);

/// Delete these blocks (case-insensitive names): references everywhere, then the definitions. Inside a change.
pub fn delete_blocks(doc: &mut Doc, names: &[String], t: &dyn Fn(&str) -> String) -> Result<Deleted, cc_cad::DocError> {
    let mut want: Vec<(String, usize)> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    for n in names {
        let b = doc.blocks.values().find(|x| x.name.to_uppercase() == n.to_uppercase()).cloned();
        let Some(b) = b else {
            failed.push((n.clone(), t("delblocks.notfound")));
            continue;
        };
        if b.xref.is_some() {
            failed.push((b.name.clone(), t("delblocks.xref")));
            continue;
        }
        if b.name.starts_with('*') {
            failed.push((b.name.clone(), t("delblocks.anonymous")));
            continue;
        }
        match want.iter_mut().find(|(k, _)| *k == b.name) {
            Some(slot) => slot.1 = 0,
            None => want.push((b.name.clone(), 0)),
        }
    }
    if want.is_empty() {
        return Ok((Vec::new(), failed));
    }
    let hit = |e: &Entity, want: &[(String, usize)]| block_of(e).is_some_and(|b| want.iter().any(|(k, _)| k == b));
    let mut spaces = vec![cc_cad::MODEL];
    spaces.extend(doc.layouts.iter().map(|l| l.space));
    for s in spaces {
        let list: Vec<Entity> = doc.space_entities(s).cloned().collect();
        for e in list {
            if hit(&e, &want) {
                if let Some(b) = block_of(&e)
                    && let Some(slot) = want.iter_mut().find(|(k, _)| k == b)
                {
                    slot.1 = slot.1.saturating_add(1);
                }
                if let Some(id) = e.id {
                    doc.remove_in(id, s)?;
                }
            }
        }
    }
    let blocks: Vec<cc_cad::Block> = doc.blocks.values().cloned().collect();
    for b in blocks {
        if want.iter().any(|(k, _)| *k == b.name) {
            continue;
        }
        if !b.entities.iter().any(|e| hit(e, &want)) {
            continue;
        }
        for e in b.entities.iter() {
            if hit(e, &want)
                && let Some(n) = block_of(e)
                && let Some(slot) = want.iter_mut().find(|(k, _)| k == n)
            {
                slot.1 = slot.1.saturating_add(1);
            }
        }
        let mut nb = b.clone();
        nb.entities = b.entities.iter().filter(|e| !hit(e, &want)).cloned().collect();
        doc.define_block(&nb)?;
    }
    let mut deleted = Vec::new();
    for (name, refs) in want {
        let used = doc.block_uses(&name) > 0 && doc.every_entity().any(|e| block_ref_any(e) == Some(name.as_str()));
        if used {
            failed.push((name, t("delblocks.inuse")));
            continue;
        }
        doc.remove_block(&name)?;
        deleted.push((name, refs));
    }
    Ok((deleted, failed))
}

/// `e.block` of any object (a reference's block, a dimension's).
fn block_ref_any(e: &Entity) -> Option<&str> {
    cc_cad::doc::block_ref(e)
}

/// The command: pick a reference, or N for a list (the window's chooser, else typed names with wildcards), repeat.
pub async fn delblocks(c: Ctx) -> R {
    loop {
        let names = deletable(&c.doc());
        if names.is_empty() {
            c.print(&c.t("delblocks.none"));
            return Ok(());
        }
        let options = vec![
            Opt { key: "N".into(), label: c.t("delblocks.opt.name"), en: None, hidden: false },
            Opt { key: "X".into(), label: c.t("delblocks.opt.exit"), en: None, hidden: false },
        ];
        let r = c.entity(c.t("delblocks.prompt"), Opts { none: true, empty: true, options, ..Opts::default() }).await?;
        let chosen: Vec<String> = match r {
            Reply::Null => return Ok(()),
            Reply::Opt(k) if k == "X" => return Ok(()),
            Reply::Opt(k) if k == "N" => {
                let title = c.t("delblocks.dialog.title");
                let picked = c.sh.host.borrow_mut().choose_blocks(&title, &names);
                let chosen = match picked {
                    crate::engine::Pick::Now(v) => v.unwrap_or_default(),
                    crate::engine::Pick::Later => match c.ask(Kind::File, title.clone(), Opts::default()).await? {
                        Reply::Choice(Some(s)) => s.split('\n').filter(|x| !x.is_empty()).map(str::to_string).collect(),
                        _ => Vec::new(),
                    },
                    crate::engine::Pick::Unsupported => {
                        let s = c.string(c.t("delblocks.typenames"), Opts::default()).await?;
                        let text = if let Reply::Str(t) = s { t } else { String::new() };
                        let pats: Vec<String> =
                            text.split(',').map(|x| cc_cad::js::trim(x).to_string()).filter(|x| !x.is_empty()).collect();
                        names
                            .iter()
                            .filter(|n| {
                                pats.iter().any(|p| {
                                    let up = |s: &str| cc_lisp::value::upper16(&cc_lisp::value::lstr(s));
                                    cc_lisp::wild::wcmatch(&up(n), &up(p)).unwrap_or(false)
                                })
                            })
                            .cloned()
                            .collect()
                    }
                };
                if chosen.is_empty() {
                    continue;
                }
                chosen
            }
            Reply::Empty { .. } => {
                c.print(&c.t("delblocks.missed"));
                continue;
            }
            Reply::Pick { id, .. } => {
                let b = c.doc().get(id).and_then(|e| block_of(e).map(str::to_string));
                let Some(b) = b else {
                    c.print(&c.t("delblocks.notblock"));
                    continue;
                };
                if !names.contains(&b) {
                    c.print(&c.tv("delblocks.cannot", &[("name", b)]));
                    continue;
                }
                vec![b]
            }
            _ => return Ok(()),
        };
        let lang = c.sh.lang.clone();
        let t = move |k: &str| lang.s(k);
        let (deleted, failed) = c.change("DELBLOCKS", |d| delete_blocks(d, &chosen, &t))?;
        for (name, n) in deleted {
            c.print(&c.tv("delblocks.deleted", &[("name", name), ("n", n.to_string())]));
        }
        for (name, reason) in failed {
            c.print(&c.tv("delblocks.failed", &[("name", name), ("reason", reason)]));
        }
    }
}
