//! The clipboard commands (commands.js): COPYCLIP CUTCLIP COPYBASE PASTECLIP PASTEORIG, over [`crate::clip`].

use std::rc::Rc;

use cc_cad::entity::Geo;
use cc_cad_geom::geom::m_translate;
use cc_cad_geom::tx::tx_entity;

use crate::clip::{Clip, base_point, make_clip, paste_clip, paste_preview};
use crate::engine::{Ctx, IdSet, Opts, R};

/// `toClipboard`: put the objects on the clipboard → true when something was copied.
fn to_clipboard(c: &Ctx, ids: &IdSet, base: Option<cc_geom::P2>) -> bool {
    let clip = make_clip(&c.doc(), &ids.0, base);
    let Some(clip) = clip else {
        c.print(&c.t(if ids.is_empty() { "cmd.copy.nothing_selected" } else { "cmd.copy.nothing_there_can_copied" }));
        return false;
    };
    let n = clip.entities.len();
    let n_blocks = clip.blocks.iter().filter(|b| !b.name.starts_with('*')).count();
    let has_blocks = !clip.blocks.is_empty();
    let skipped = clip.skipped;
    let shared = {
        let sh = c.sh();
        let mut host = sh.host.borrow_mut();
        let mut st = sh.st.borrow_mut();
        st.clipboard.put(clip, |c| host.clip_put(c))
    };
    let objects = c.tv("cmd.n.objects", &[("n", n.to_string())]);
    let mut parts = vec![if has_blocks {
        c.tv("cmd.copy.copied.blocks", &[("objects", objects), ("blocks", c.tv("cmd.n.blockdefs", &[("n", n_blocks.to_string())]))])
    } else {
        c.tv("cmd.copy.copied", &[("objects", objects)])
    }];
    if skipped > 0 {
        parts.push(c.tv("cmd.copy.viewports_stayed", &[("n", skipped.to_string())]));
    }
    if !shared {
        parts.push(c.t("cmd.copy.too_big"));
    }
    c.print(&parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" "));
    true
}

pub async fn copyclip(c: Ctx) -> R {
    let ids = c.select(Some(c.t("cmd.copyclip.select_objects_copy")), false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    to_clipboard(&c, &ids, None);
    Ok(())
}

pub async fn cutclip(c: Ctx) -> R {
    let ids = c.select(Some(c.t("cmd.cutclip.select_objects_cut")), false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    if !to_clipboard(&c, &ids, None) {
        return Ok(());
    }
    c.change("CUTCLIP", |d| {
        for id in &ids.0 {
            if d.get(*id).is_some_and(|e| !matches!(e.geo, Geo::Viewport(_))) {
                d.remove(*id)?;
            }
        }
        Ok(())
    })?;
    c.set_selection(&[]);
    Ok(())
}

pub async fn copybase(c: Ctx) -> R {
    let base = c.must_point(c.t("cmd.stretch.base"), Opts::default()).await?;
    let ids = c.select(Some(c.t("cmd.copyclip.select_objects_copy")), false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    to_clipboard(&c, &ids, Some(base));
    Ok(())
}

/// `fromClipboard`: the clipboard's contents, or None (and why).
fn from_clipboard(c: &Ctx) -> Option<Clip> {
    let raw = c.sh().host.borrow().clip_get();
    let got = c.sh().st.borrow().clipboard.get(raw, &c.sh().lang);
    if let Some(e) = got.error {
        c.print(&c.tv("cmd.paste.nothing", &[("why", e)]));
        return None;
    }
    let Some(clip) = got.clip else {
        c.print(&c.t("cmd.copybase.clipboard_empty_copy_objects"));
        return None;
    };
    if let Some(w) = got.warning {
        c.print(&c.tv("cmd.paste.shared_unreadable", &[("why", w)]));
    }
    Some(clip)
}

fn finish_paste(c: &Ctx, clip: &Clip, dx: f64, dy: f64, label: &str) -> R {
    let lang = c.sh().lang.clone();
    let r = c.change(label, |d| paste_clip(d, clip, dx, dy, &lang))?;
    c.set_selection(&r.ids);
    let objects = c.tv("cmd.n.objects", &[("n", r.ids.len().to_string())]);
    if r.layers.is_empty() {
        c.print(&c.tv("cmd.paste.pasted", &[("objects", objects)]));
    } else {
        let mut list = r.layers.iter().take(12).cloned().collect::<Vec<_>>().join(", ");
        if r.layers.len() > 12 {
            list.push_str(" …");
        }
        c.print(&c.tv("cmd.paste.pasted.layers", &[("objects", objects), ("n", r.layers.len().to_string()), ("layers", list)]));
    }
    let renamed: Vec<String> = r
        .renamed
        .iter()
        .filter(|(a, _)| !a.starts_with('*'))
        .map(|(a, b)| c.tv("cmd.renamed_as", &[("from", a.clone()), ("to", b.clone())]))
        .collect();
    if !renamed.is_empty() {
        c.print(&c.tv("cmd.paste.renamed", &[("list", renamed.join(", "))]));
    }
    if !r.note.is_empty() {
        c.print(&r.note);
    }
    Ok(())
}

pub async fn pasteclip(c: Ctx) -> R {
    let Some(clip) = from_clipboard(&c) else { return Ok(()) };
    let mode = c.sh().host.borrow().paste_base();
    let base = base_point(&clip, mode.as_deref());
    let look = Rc::new(paste_preview(&clip, 20_000));
    c.st().last_point = Some(base);
    c.preview(Some(Rc::new(move |_, cur| look.iter().map(|e| tx_entity(e, &m_translate(cur.x - base.x, cur.y - base.y))).collect())));
    let p = c.must_point(c.t("cmd.pasteclip.specify_insertion_point"), Opts::default()).await?;
    c.preview(None);
    finish_paste(&c, &clip, p.x - base.x, p.y - base.y, "PASTECLIP")
}

pub async fn pasteorig(c: Ctx) -> R {
    let Some(clip) = from_clipboard(&c) else { return Ok(()) };
    finish_paste(&c, &clip, 0.0, 0.0, "PASTEORIG")
}
