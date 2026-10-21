//! PUBLISH (layertools.js, 0.4 C36): several or all sheets plotted into one PDF, a page and a bookmark per sheet
//! (cc-io-cad `plot::to_pdf_pages`), with the ink and pen the Plot dialog last used.

use cc_cad::js;
use cc_io_cad::plot::{display_list, layout_sheet, to_pdf_pages};

use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::Def;

/// layertools.js `listOf`: the first eight names, then " …".
fn list_of(names: &[String]) -> String {
    let mut s = names.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
    if names.len() > 8 {
        s.push_str(" …");
    }
    s
}

pub async fn publish(c: Ctx) -> R {
    let all: Vec<String> = c.doc().layouts.iter().map(|l| l.name.clone()).collect();
    if all.is_empty() {
        c.print(&c.t("publish.nosheets"));
        return Ok(());
    }
    let options = vec![c.opt("A", &c.t("opt.all")), c.opt("S", &c.t("opt.some"))];
    let k = c
        .keyword(
            c.tv("publish.which", &[("n", all.len().to_string())]),
            Opts { options, default: Some(Def::Str("A".into())), ..Opts::default() },
        )
        .await?;
    let o = match k {
        Reply::Opt(o) | Reply::Str(o) => o,
        _ => return Ok(()),
    };
    if o.is_empty() {
        return Ok(());
    }
    // indices into the layouts, in the drawing's order
    let mut pick: Vec<usize> = (0..all.len()).collect();
    if o == "S" {
        let r = c.string(c.t("publish.names"), Opts::default()).await?;
        let s = if let Reply::Str(s) = r { s } else { String::new() };
        let want: Vec<String> = s.split(',').map(|x| js::trim(x).to_uppercase()).filter(|x| !x.is_empty()).collect();
        pick = all.iter().enumerate().filter(|(_, n)| want.contains(&n.to_uppercase())).map(|(i, _)| i).collect();
        let missing: Vec<String> = want.iter().filter(|w| !all.iter().any(|n| n.to_uppercase() == **w)).cloned().collect();
        if !missing.is_empty() {
            c.print(&c.tv("publish.unknown", &[("list", missing.join(", "))]));
        }
        if pick.is_empty() {
            return Ok(());
        }
    }
    if !c.sh().host.borrow().can_save_blob() {
        c.print(&c.t("count.nodownload"));
        return Ok(());
    }
    let (ink, pen, date) = {
        let h = c.sh().host.borrow();
        let (ink, pen) = h.plot_prefs().unwrap_or_else(|| ("mono".into(), 0.25));
        (ink, pen, h.now_ms().map_or(0.0, f64::floor))
    };
    let names: Vec<String> = pick.iter().filter_map(|i| all.get(*i).cloned()).collect();
    let (pdf, name) = {
        let d = c.doc();
        let lays: Vec<&cc_cad::Layout> = pick.iter().filter_map(|i| d.layouts.get(*i)).collect();
        let doc: &cc_cad::Doc = &d;
        let mono = ink != "color";
        let one = |l: &&cc_cad::Layout| display_list(doc, &layout_sheet(doc, l), mono);
        // each sheet's display list on its own thread natively (the drawing is only read)
        #[cfg(not(target_arch = "wasm32"))]
        let lists: Vec<_> = {
            use rayon::prelude::*;
            lays.par_iter().map(one).collect()
        };
        #[cfg(target_arch = "wasm32")]
        let lists: Vec<_> = lays.iter().map(one).collect();
        let name = d.meta.get("name").filter(|v| js::truthy(Some(v))).map_or_else(|| "Drawing1".into(), js::to_js_string);
        (to_pdf_pages(&lists, pen, &name, &names, date, true), name)
    };
    let file = format!("{name}.pdf");
    c.sh().host.borrow_mut().save_blob(&file, &pdf.bytes, "application/pdf");
    c.print(&c.tv("publish.done", &[("n", names.len().to_string()), ("file", file), ("list", list_of(&names))]));
    if pdf.missing > 0 {
        c.print(&c.tv("publish.missing", &[("n", pdf.missing.to_string())]));
    }
    Ok(())
}
