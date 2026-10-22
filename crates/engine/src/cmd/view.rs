//! ZOOM (Extents / All / Window / Previous / Scale / Center, a typed factor), UNDO, REDO.

use std::rc::Rc;

use cc_cad::js;
use cc_cad_geom::units::parse_length;
use cc_geom::P2;

use crate::cmd::draw::rect;
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::{Def, fmt, zoom_factor};

pub async fn zoom(c: Ctx) -> R {
    let options = vec![
        c.opt("E", &c.t("opt.extents")),
        c.opt("A", &c.t("opt.all")),
        c.opt("W", &c.t("opt.window")),
        c.opt("P", &c.t("opt.previous")),
        c.opt("S", &c.t("opt.zscale")),
        c.opt("C", &c.t("opt.center")),
    ];
    let k = c.point(c.t("cmd.zoom.prompt"), Opts { options, none: true, accept: Some(zoom_factor), ..Opts::default() }).await?;
    let host = || c.sh().host.borrow_mut();
    match &k {
        Reply::Null => {
            host().zoom_extents();
            return Ok(());
        }
        Reply::Opt(o) if o == "E" || o == "A" => {
            host().zoom_extents();
            return Ok(());
        }
        Reply::Opt(o) if o == "P" => {
            if !host().zoom_previous() {
                c.print(&c.t("cmd.zoom.noprev"));
            }
            return Ok(());
        }
        Reply::Typed(f) => {
            host().zoom_by(*f);
            return Ok(());
        }
        Reply::Opt(o) if o == "S" => {
            let v = c.string(c.t("cmd.zoom.factor"), Opts::default()).await?;
            let f = match &v {
                Reply::Str(s) => zoom_factor(s),
                _ => None,
            };
            let Some(f) = f else {
                c.print(&c.t("cmd.zoom.factor.bad"));
                return Ok(());
            };
            host().zoom_by(f);
            return Ok(());
        }
        Reply::Opt(o) if o == "C" => {
            let p = c.must_point(c.t("cmd.zoom.center"), Opts::default()).await?;
            let cur = c.sh().host.borrow().view_rect();
            let default = cur.map(|(_, h)| Def::Str(fmt(h)));
            let v = match c.string(c.t("cmd.zoom.height"), Opts { default, ..Opts::default() }).await? {
                Reply::Str(s) => s,
                _ => String::new(),
            };
            let lunits = c.doc().meta.get("lunits").map_or(2.0, js::to_number);
            let fx = if js::trim(&v).ends_with(['x', 'X']) { zoom_factor(&v) } else { None };
            let h = match fx {
                Some(f) => cur.map(|(_, h)| h / f),
                None => parse_length(&v, lunits),
            };
            let Some(h) = h.filter(|h| *h > 0.0) else {
                c.print(&c.t("cmd.zoom.factor.bad"));
                return Ok(());
            };
            let aspect = cur.map_or(1.0, |(w, hh)| w / hh);
            let a = P2::new(p.x - h * aspect / 2.0, p.y - h / 2.0);
            let b = P2::new(p.x + h * aspect / 2.0, p.y + h / 2.0);
            host().zoom_window(a, b, true);
            return Ok(());
        }
        _ => {}
    }
    let a = if k.option() == Some("W") {
        c.must_point(c.t("cmd.mtext.first"), Opts::default()).await?
    } else {
        match k.point() {
            Some(p) => p,
            None => return Ok(()),
        }
    };
    c.preview(Some(Rc::new(move |_, cur| vec![rect(a, cur)])));
    let b = c.must_point(c.t("cmd.mtext.corner"), Opts { base: Some(a), ..Opts::default() }).await?;
    c.sh().host.borrow_mut().zoom_window(a, b, false);
    Ok(())
}

pub async fn undo(c: Ctx) -> R {
    let l = c.sh().doc.borrow_mut().undo();
    let msg = match l {
        Some(l) if !l.is_empty() => format!("Undid {l}"),
        _ => c.t("cmd.undo.nothing_undo"),
    };
    c.print(&msg);
    Ok(())
}

pub async fn redo(c: Ctx) -> R {
    let l = c.sh().doc.borrow_mut().redo();
    let msg = match l {
        Some(l) if !l.is_empty() => format!("Redid {l}"),
        _ => c.t("cmd.redo.nothing_redo"),
    };
    c.print(&msg);
    Ok(())
}

/// PROPERTIES (PR, PROPS, CH, MO, DDMODIFY): open the Properties panel.
pub async fn properties(c: Ctx) -> R {
    c.sh().host.borrow_mut().props(true);
    Ok(())
}
/// PROPERTIESCLOSE (PRCLOSE): fold it away.
pub async fn properties_close(c: Ctx) -> R {
    c.sh().host.borrow_mut().props(false);
    Ok(())
}
/// QPMODE (QP): Quick Properties by the cursor on / off.
pub async fn qpmode(c: Ctx) -> R {
    c.sh().host.borrow_mut().quick_props();
    Ok(())
}

/// _PROPPICK (hidden): the Properties panel's pick button beside a point — asks for the point, with snaps, and hands
/// it to the panel (propcmds.js).
pub async fn prop_pick(c: Ctx) -> R {
    let what = c.sh().host.borrow().prop_pick_label().unwrap_or_default();
    let p = c.must_point(c.tv("props.pick.prompt", &[("what", what)]), crate::engine::Opts::default()).await?;
    c.sh().host.borrow_mut().prop_picked(p);
    Ok(())
}

/// PLOT (PRINT, PUBLISH, EXPORTPDF): the Plot dialog (the window's own; elsewhere plotting needs the app).
pub async fn plot(c: Ctx) -> R {
    let has = c.sh().host.borrow_mut().plot();
    if !has {
        c.print(&c.t("cmd.plot.plotting_needs_browser"));
    }
    Ok(())
}

/// _PLOTWIN (hidden): the Plot dialog's Pick — two corners on the model (cadcraft.js pickPlotWindow), then the dialog
/// opens again with that window (Esc: as it was).
pub async fn plot_window(c: Ctx) -> R {
    let picked = async {
        let a = c.must_point(c.t("app.specify_first_corner_area"), crate::engine::Opts::default()).await?;
        c.preview(Some(std::rc::Rc::new(move |_, cur| vec![crate::cmd::draw::rect(a, cur)])));
        let b = c
            .must_point(c.t("app.specify_opposite_corner"), crate::engine::Opts { base: Some(a), ..crate::engine::Opts::default() })
            .await?;
        c.preview(None);
        Ok::<_, crate::prompt::CmdError>((a, b))
    }
    .await;
    let w = match picked {
        Ok((a, b)) if (b.x - a.x).abs() > 1e-9 && (b.y - a.y).abs() > 1e-9 => Some(cc_cad_geom::geom::Bounds {
            min_x: cc_cad::js::min(a.x, b.x),
            min_y: cc_cad::js::min(a.y, b.y),
            max_x: cc_cad::js::max(a.x, b.x),
            max_y: cc_cad::js::max(a.y, b.y),
        }),
        _ => None,
    };
    c.sh().host.borrow_mut().plot_window(w);
    if let Err(crate::prompt::CmdError::Cancelled) = picked {
        return Err(crate::prompt::CmdError::Cancelled);
    }
    Ok(())
}
