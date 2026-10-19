//! Layer commands (layertools.js): LAYISO LAYUNISO LAYOFF LAYFRZ LAYMCUR LAYON LAYTHW LAYMRG LAYDEL RENAME, and
//! LINETYPE LTSCALE LWEIGHT. Every one is one undo step (the layer table and the current layer). Not ported here:
//! COUNT, XATTACH / XREF, SHEETSET, PUBLISH (they need files or the browser).

use cc_cad::doc::{Block, LayerState};
use cc_cad::entity::Entity;
use cc_cad::{MODEL, js};
use cc_cad_geom::linetypes::{LINEWEIGHTS, standard_names};
use serde_json::Value;

use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def;

/// What the last LAYISO did (`doc.layIso`): the layers it turned off and the current layer it changed.
#[derive(Clone, Debug, PartialEq)]
pub struct LayIso {
    pub off: Vec<String>,
    pub current: Option<String>,
}

fn layer_of(c: &Ctx, id: u64) -> Option<String> {
    c.doc().get(id).and_then(|e| e.layer.clone())
}
fn list_of(names: &[String]) -> String {
    let mut s = names.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 8 {
        s.push_str(" …");
    }
    s
}

/// `changeLayers`: one undo step when `edit` changed a copy of the layer table → (changed, result).
fn change_layers<T>(c: &Ctx, label: &str, edit: impl FnOnce(&mut LayerState) -> T) -> R<(bool, T)> {
    let before = c.doc().layer_state();
    let mut state = before.clone();
    let r = edit(&mut state);
    if state == before {
        return Ok((false, r));
    }
    c.change(label, |d| d.set_layer_state(&state))?;
    Ok((true, r))
}

pub async fn layiso(c: Ctx) -> R {
    let ids = c.select(Some(c.t("cmd.layiso.select")), false).await?;
    if ids.is_empty() {
        return Ok(());
    }
    let mut names: Vec<String> = Vec::new();
    for id in &ids.0 {
        if let Some(n) = layer_of(&c, *id)
            && !names.contains(&n)
        {
            names.push(n);
        }
    }
    let mut iso = None;
    let (_, off) = change_layers(&c, "LAYISO", |s| {
        let mut off = Vec::new();
        for l in &mut s.list {
            if names.contains(&l.name) {
                l.on = true;
                continue;
            }
            if l.on {
                l.on = false;
                off.push(l.name.clone());
            }
        }
        let was = s.current.clone();
        if !names.contains(&s.current) {
            s.current = names.first().cloned().unwrap_or_default();
        }
        let cur = s.current.clone();
        if let Some(t) = s.list.iter_mut().find(|l| l.name == cur) {
            t.frozen = false;
        }
        iso = Some(LayIso { off: off.clone(), current: if was != s.current { Some(was) } else { None } });
        off
    })?;
    c.st().memo.lay_iso = iso;
    c.print(&c.tv("cmd.layiso.done", &[("n", names.len().to_string()), ("list", list_of(&names)), ("off", off.len().to_string())]));
    Ok(())
}

pub async fn layuniso(c: Ctx) -> R {
    let Some(iso) = c.st().memo.lay_iso.take() else {
        c.print(&c.t("cmd.layuniso.none"));
        return Ok(());
    };
    let (_, n) = change_layers(&c, "LAYUNISO", |s| {
        let mut n = 0usize;
        for l in &mut s.list {
            if iso.off.contains(&l.name) && !l.on {
                l.on = true;
                n = n.saturating_add(1);
            }
        }
        if let Some(back) = &iso.current
            && s.list.iter().any(|l| &l.name == back && l.on && !l.frozen)
        {
            s.current = back.clone();
        }
        n
    })?;
    c.print(&c.tv("cmd.layuniso.done", &[("n", n.to_string())]));
    Ok(())
}

/// LAYOFF / LAYFRZ: pick objects one by one; each pick is its own undo step, and U takes the last one back.
pub async fn by_pick(c: Ctx, name: &'static str, freeze: bool, label: &'static str) -> R {
    let mut done: Vec<(String, usize)> = Vec::new();
    loop {
        let options = if done.is_empty() { Vec::new() } else { vec![c.opt("U", &c.t("opt.undo"))] };
        let pick = c.entity(c.t(&format!("cmd.{label}.pick")), Opts { none: true, options, ..Opts::default() }).await?;
        let id = match pick {
            Reply::Opt(k) if k == "U" => {
                if let Some((layer, step)) = done.pop()
                    && c.doc().undo_stack.len() == step
                {
                    c.sh().doc.borrow_mut().undo();
                    c.print(&c.tv("cmd.layer.undone", &[("layer", layer)]));
                }
                continue;
            }
            Reply::Pick { id, .. } => id,
            _ => return Ok(()),
        };
        let Some(layer) = layer_of(&c, id) else { continue };
        if layer == c.doc().current.layer {
            c.print(&c.tv(&format!("cmd.{label}.current"), &[("layer", layer)]));
            continue;
        }
        let (changed, ()) = change_layers(&c, name, |s| {
            if let Some(l) = s.list.iter_mut().find(|x| x.name == layer) {
                if freeze {
                    l.frozen = true;
                } else {
                    l.on = false;
                }
            }
        })?;
        if !changed {
            c.print(&c.tv(&format!("cmd.{label}.already"), &[("layer", layer)]));
            continue;
        }
        done.push((layer.clone(), c.doc().undo_stack.len()));
        c.set_selection(&[]);
        c.print(&c.tv(&format!("cmd.{label}.done"), &[("layer", layer)]));
    }
}

pub async fn laymcur(c: Ctx) -> R {
    let pre: Vec<u64> = {
        let d = c.doc();
        c.sh().host.borrow().selection().into_iter().filter(|id| d.get(*id).is_some()).collect()
    };
    let id = if pre.len() == 1 {
        pre.first().copied()
    } else {
        match c.entity(c.t("cmd.laymcur.pick"), Opts { none: true, ..Opts::default() }).await? {
            Reply::Pick { id, .. } => Some(id),
            _ => None,
        }
    };
    let Some(layer) = id.filter(|i| *i != 0).and_then(|i| layer_of(&c, i)) else { return Ok(()) };
    change_layers(&c, "LAYMCUR", |s| {
        if let Some(l) = s.list.iter_mut().find(|x| x.name == layer) {
            l.on = true;
            l.frozen = false;
            s.current = layer.clone();
        }
    })?;
    c.print(&c.tv("cmd.laymcur.done", &[("layer", layer)]));
    Ok(())
}

pub async fn all_layers(c: Ctx, name: &'static str, on: bool, label: &'static str) -> R {
    let (_, n) = change_layers(&c, name, |s| {
        let mut k = 0usize;
        for l in &mut s.list {
            if on && !l.on {
                l.on = true;
                k = k.saturating_add(1);
            } else if !on && l.frozen {
                l.frozen = false;
                k = k.saturating_add(1);
            }
        }
        k
    })?;
    if on {
        c.st().memo.lay_iso = None;
    }
    c.print(&c.tv(&format!("cmd.{label}.done"), &[("n", n.to_string())]));
    Ok(())
}

/// A layer: an object picked on it, or N and its name. None = Enter.
async fn layer_by_pick(c: &Ctx, prompt: String) -> R<Option<String>> {
    loop {
        let p = c.entity(prompt.clone(), Opts { none: true, options: vec![c.opt("N", &c.t("opt.name"))], ..Opts::default() }).await?;
        match p {
            Reply::Opt(k) if k == "N" => {
                let r = c.string(c.t("cmd.layer.name"), Opts::default()).await?;
                let s = if let Reply::Str(s) = r { js::trim(&s).to_string() } else { String::new() };
                if s.is_empty() {
                    continue;
                }
                let hit = c.doc().layers.keys().find(|n| n.to_uppercase() == s.to_uppercase()).cloned();
                if let Some(h) = hit {
                    return Ok(Some(h));
                }
                c.print(&c.tv("cmd.layer.none", &[("layer", s)]));
            }
            Reply::Pick { id, .. } => {
                if let Some(n) = layer_of(c, id) {
                    return Ok(Some(n));
                }
            }
            _ => return Ok(None),
        }
    }
}

/// `forEachObject`: every object in the model, on every sheet and in every block; `f` → Some(next) changes it,
/// Some(None) erases it.
fn for_each_object(d: &mut cc_cad::Doc, f: &mut dyn FnMut(&Entity) -> Option<Option<Entity>>) -> Result<(), cc_cad::DocError> {
    let mut maps = vec![MODEL];
    maps.extend(d.layouts.iter().map(|l| l.space));
    for s in maps {
        let list: Vec<Entity> = d.space_entities(s).cloned().collect();
        for e in list {
            let id = e.id.unwrap_or(0);
            match f(&e) {
                Some(Some(next)) => {
                    d.replace_in(id, next, s)?;
                }
                Some(None) => d.remove_in(id, s)?,
                None => {}
            }
        }
    }
    let blocks: Vec<Block> = d.blocks.values().cloned().collect();
    for b in blocks {
        let mut changed = false;
        let mut ents = Vec::new();
        for e in &b.entities {
            match f(e) {
                Some(Some(next)) => {
                    ents.push(next);
                    changed = true;
                }
                Some(None) => changed = true,
                None => ents.push(e.clone()),
            }
        }
        if changed {
            let mut nb = b.clone();
            nb.entities = ents;
            d.define_block(&nb)?;
        }
    }
    Ok(())
}

fn yes_no(c: &Ctx) -> Vec<crate::input::Opt> {
    vec![c.opt("Y", &c.t("opt.yes")), c.opt("N", &c.t("opt.no"))]
}
fn is_yes(r: &Reply) -> bool {
    matches!(r, Reply::Opt(x) | Reply::Str(x) if x == "Y")
}

pub async fn laymrg(c: Ctx) -> R {
    let mut from: Vec<String> = Vec::new();
    loop {
        let prompt = if from.is_empty() { c.t("cmd.laymrg.pick") } else { c.tv("cmd.laymrg.more", &[("list", list_of(&from))]) };
        let Some(n) = layer_by_pick(&c, prompt).await? else { break };
        if !from.contains(&n) {
            from.push(n);
        }
    }
    if from.is_empty() {
        return Ok(());
    }
    let Some(to) = layer_by_pick(&c, c.t("cmd.laymrg.target")).await? else { return Ok(()) };
    let src: Vec<String> = from.iter().filter(|x| **x != to).cloned().collect();
    let ok = c
        .keyword(
            c.tv("cmd.laymrg.confirm", &[("n", src.len().to_string()), ("list", list_of(&src)), ("to", to.clone())]),
            Opts { options: yes_no(&c), default: Some(Def::Str("N".into())), ..Opts::default() },
        )
        .await?;
    if !is_yes(&ok) {
        c.print(&c.t("cmd.layer.nothing"));
        return Ok(());
    }
    if src.is_empty() {
        c.print(&c.t("cmd.laymrg.same"));
        return Ok(());
    }
    if c.doc().layers.get(&to).is_none() {
        c.print(&c.tv("cmd.layer.none", &[("layer", to)]));
        return Ok(());
    }
    if src.iter().any(|n| n == "0") {
        c.print(&c.t("cmd.laymrg.zero"));
        return Ok(());
    }
    let mut moved = 0usize;
    c.change("LAYMRG", |d| {
        for_each_object(d, &mut |e| {
            if e.layer.as_ref().is_some_and(|l| src.contains(l)) {
                moved = moved.saturating_add(1);
                let mut n = e.clone();
                n.layer = Some(to.clone());
                Some(Some(n))
            } else {
                None
            }
        })?;
        let mut s = d.layer_state();
        s.list.retain(|l| !src.contains(&l.name));
        if src.contains(&s.current) {
            s.current = to.clone();
        }
        d.set_layer_state(&s)
    })?;
    c.print(&c.tv("cmd.laymrg.done", &[("n", moved.to_string()), ("layers", src.len().to_string()), ("to", to)]));
    Ok(())
}

pub async fn laydel(c: Ctx) -> R {
    let Some(n) = layer_by_pick(&c, c.t("cmd.laydel.pick")).await? else { return Ok(()) };
    let count = {
        let d = c.doc();
        let on = |e: &Entity| e.layer.as_deref() == Some(n.as_str());
        d.every_entity()
            .filter(|e| on(e))
            .count()
            .saturating_add(d.blocks.values().map(|b| b.entities.iter().filter(|e| on(e)).count()).fold(0usize, usize::saturating_add))
    };
    let ok = c
        .keyword(
            c.tv("cmd.laydel.confirm", &[("layer", n.clone()), ("n", count.to_string())]),
            Opts { options: yes_no(&c), default: Some(Def::Str("N".into())), ..Opts::default() },
        )
        .await?;
    if !is_yes(&ok) {
        c.print(&c.t("cmd.layer.nothing"));
        return Ok(());
    }
    if n == "0" || n.eq_ignore_ascii_case("defpoints") {
        c.print(&c.tv("cmd.laydel.fixed", &[("layer", n)]));
        return Ok(());
    }
    if n == c.doc().current.layer {
        c.print(&c.tv("cmd.laydel.current", &[("layer", n)]));
        return Ok(());
    }
    if c.doc().layers.get(&n).is_none() {
        c.print(&c.tv("cmd.layer.none", &[("layer", n)]));
        return Ok(());
    }
    let mut erased = 0usize;
    c.change("LAYDEL", |d| {
        for_each_object(d, &mut |e| {
            if e.layer.as_deref() == Some(n.as_str()) {
                erased = erased.saturating_add(1);
                Some(None)
            } else {
                None
            }
        })?;
        let mut s = d.layer_state();
        s.list.retain(|l| l.name != n);
        d.set_layer_state(&s)
    })?;
    c.print(&c.tv("cmd.laydel.done", &[("layer", n), ("n", erased.to_string())]));
    Ok(())
}

pub async fn rename(c: Ctx) -> R {
    let k = c
        .keyword(
            c.t("cmd.rename.type"),
            Opts { options: vec![c.opt("LA", &c.t("opt.layer")), c.opt("B", &c.t("opt.block"))], ..Opts::default() },
        )
        .await?;
    let Reply::Opt(kind) = k else { return Ok(()) };
    let names: Vec<String> = if kind == "LA" {
        c.doc().layers.keys().cloned().collect()
    } else {
        c.doc().blocks.keys().filter(|n| !n.starts_with('*')).cloned().collect()
    };
    let r = c.string(c.t("cmd.rename.old"), Opts::default()).await?;
    let old_s = if let Reply::Str(s) = r { js::trim(&s).to_string() } else { String::new() };
    let Some(from) = names.iter().find(|n| n.to_uppercase() == old_s.to_uppercase()).cloned() else {
        if !old_s.is_empty() {
            c.print(&if kind == "LA" {
                c.tv("cmd.layer.none", &[("layer", old_s)])
            } else {
                c.tv("cmd.rename.noblock", &[("name", old_s)])
            });
        }
        return Ok(());
    };
    let r = c.string(c.tv("cmd.rename.new", &[("old", from.clone())]), Opts::default()).await?;
    let to = if let Reply::Str(s) = r { js::trim(&s).to_string() } else { String::new() };
    if to.is_empty() || to == from {
        return Ok(());
    }
    if kind == "LA" {
        if from == "0" {
            c.print(&c.t("cmd.rename.zero"));
            return Ok(());
        }
        if names.iter().any(|n| n.to_uppercase() == to.to_uppercase() && *n != from) {
            c.print(&c.tv("cmd.rename.taken", &[("name", to)]));
            return Ok(());
        }
        let mut patch = cc_cad::Obj::new();
        patch.insert("name".into(), Value::String(to.clone()));
        c.sh().doc.borrow_mut().set_layer(&from, &patch, false).map_err(crate::engine::doc_err)?;
        c.print(&c.tv("cmd.rename.done", &[("old", from), ("name", to)]));
    } else {
        let b = c.doc().blocks.get(&from).cloned();
        let Some(b) = b else {
            c.print(&c.tv("cmd.rename.noblock", &[("name", from)]));
            return Ok(());
        };
        if c.doc().blocks.get(&to).is_some() || to.starts_with('*') {
            c.print(&c.tv("cmd.rename.taken", &[("name", to)]));
            return Ok(());
        }
        c.change("RENAME", |d| {
            let mut nb = b.clone();
            nb.name = to.clone();
            d.define_block(&nb)?;
            for_each_object(d, &mut |e| match &e.geo {
                cc_cad::Geo::Insert(i) if i.block == from => {
                    let mut n = e.clone();
                    if let cc_cad::Geo::Insert(x) = &mut n.geo {
                        x.block = to.clone();
                    }
                    Some(Some(n))
                }
                _ => None,
            })?;
            d.remove_block(&from)
        })?;
        c.print(&c.tv("cmd.rename.done", &[("old", from), ("name", to)]));
    }
    Ok(())
}

/// linetypes.js `linetypeNames(doc)`.
fn linetype_names(c: &Ctx) -> Vec<String> {
    let mut out = vec!["Continuous".to_string()];
    for n in c.doc().linetypes.keys() {
        if !out.iter().any(|x| x.to_uppercase() == n.to_uppercase()) {
            out.push(n.clone());
        }
    }
    for n in standard_names() {
        if !out.iter().any(|x| x.to_uppercase() == n) {
            out.push(n);
        }
    }
    out
}

/// linetypes.js `ltLabel`.
pub fn lt_label(c: &Ctx, n: Option<&str>) -> String {
    match n {
        None => c.t("lt.bylayer"),
        Some(s) if s.eq_ignore_ascii_case("bylayer") => c.t("lt.bylayer"),
        Some(s) if s.eq_ignore_ascii_case("byblock") => c.t("lt.byblock"),
        Some(s) if s.is_empty() || s.eq_ignore_ascii_case("continuous") => c.t("lt.continuous"),
        Some(s) => s.to_string(),
    }
}
/// linetypes.js `lwLabel`.
pub fn lw_label(c: &Ctx, w: Option<f64>) -> String {
    match w {
        None => c.t("lt.bylayer"),
        Some(-1.0) => c.t("lt.bylayer"),
        Some(-2.0) => c.t("lt.byblock"),
        Some(-3.0) => c.t("lw.default"),
        Some(v) => c.tv("lw.mm", &[("mm", js::to_fixed(v, 2))]),
    }
}

pub async fn linetype(c: Ctx) -> R {
    let names = linetype_names(&c);
    loop {
        let cur = c.doc().current.linetype.clone();
        let label = if cur.as_deref().is_some_and(|s| !s.is_empty()) { lt_label(&c, cur.as_deref()) } else { "ByLayer".into() };
        let r = c.string(c.tv("cmd.linetype.prompt", &[("cur", label)]), Opts::default()).await?;
        let v = if let Reply::Str(s) = r { js::trim(&s).to_string() } else { String::new() };
        if v.is_empty() {
            return Ok(());
        }
        if v == "?" {
            let mut all = vec!["ByLayer".to_string(), "ByBlock".to_string()];
            all.extend(names.iter().cloned());
            c.print(&all.join(", "));
            continue;
        }
        let hit: Option<Option<String>> = if v.eq_ignore_ascii_case("bylayer") {
            Some(None)
        } else if v.eq_ignore_ascii_case("byblock") {
            Some(Some("ByBlock".into()))
        } else {
            names.iter().find(|n| n.to_uppercase() == v.to_uppercase()).map(|n| Some(n.clone()))
        };
        let Some(hit) = hit else {
            c.print(&c.tv("cmd.linetype.unknown", &[("name", v)]));
            continue;
        };
        c.sh().doc.borrow_mut().current.linetype = hit.clone();
        let label = match &hit {
            Some(h) => lt_label(&c, Some(h)),
            None => "ByLayer".into(),
        };
        c.print(&c.tv("cmd.linetype.done", &[("name", label)]));
        return Ok(());
    }
}

pub async fn ltscale(c: Ctx) -> R {
    let cur = c.doc().meta.get("ltscale").and_then(Value::as_f64).filter(|v| *v > 0.0).unwrap_or(1.0);
    let v = c
        .number(c.tv("cmd.ltscale.prompt", &[("cur", js::num_to_string(cur))]), Opts { default: Some(Def::Num(cur)), ..Opts::default() })
        .await?;
    let Reply::Num(v) = v else { return Ok(()) };
    if !(v > 0.0) {
        c.print(&c.t("cmd.ltscale.positive"));
        return Ok(());
    }
    if v != cur {
        let mut patch = cc_cad::Obj::new();
        patch.insert("ltscale".into(), cc_cad::num(v));
        c.change("LTSCALE", |d| d.set_meta(&patch))?;
    }
    c.print(&c.tv("cmd.ltscale.done", &[("n", js::num_to_string(v))]));
    Ok(())
}

pub async fn lweight(c: Ctx) -> R {
    loop {
        let cur = c.doc().current.lineweight;
        let r = c.string(c.tv("cmd.lweight.prompt", &[("cur", lw_label(&c, cur))]), Opts::default()).await?;
        let s = if let Reply::Str(s) = r { js::trim(&s).to_string() } else { String::new() };
        if s.is_empty() {
            return Ok(());
        }
        let v: Option<Option<f64>> = if s.eq_ignore_ascii_case("bylayer") {
            Some(None)
        } else if s.eq_ignore_ascii_case("byblock") {
            Some(Some(-2.0))
        } else if s.eq_ignore_ascii_case("default") {
            Some(Some(-3.0))
        } else {
            let n = crate::input::parse_float(&s);
            if !n.is_finite() || n < 0.0 || n > 2.11 {
                None
            } else {
                let mut best = LINEWEIGHTS[0];
                for w in LINEWEIGHTS {
                    if (w - n).abs() < (best - n).abs() {
                        best = w;
                    }
                }
                Some(Some(best))
            }
        };
        let Some(v) = v else {
            c.print(&c.tv("cmd.lweight.unknown", &[("value", s)]));
            continue;
        };
        c.sh().doc.borrow_mut().current.lineweight = v;
        c.print(&c.tv("cmd.lweight.done", &[("lw", lw_label(&c, v))]));
        return Ok(());
    }
}
