//! The Studio Live commands of the drawing (0.5 cadcraft/dev/live.js `attachUi`, LV.9): LIVEHOLD (hold a layer or an
//! area while you work there), LIVELOCK / LIVEUNLOCK (the host's padlocks on objects or a layer). They read the session
//! from `Doc::live_info` and ask for holds and locks through `Doc::live_reqs`; the window (ui-egui cadlive.rs) sends
//! them. Outside a session they are unknown commands, as in 0.5 (registered only while live).

use cc_cad::doc::LiveReq;

use crate::cmd::block::s_reply;
use crate::engine::{Ctx, Opts, R, Reply};
use crate::input::{Def, Opt};

const HOLD_MAX: usize = 500;

fn opt(c: &Ctx, key: &str, label_key: &str, en: &str) -> Opt {
    Opt { key: key.into(), label: c.t(label_key), en: Some(en.into()), hidden: false }
}

/// The keyword answered (the option's key, upper case).
fn key_of(r: &Reply) -> String {
    match r {
        Reply::Opt(s) | Reply::Str(s) => s.trim().to_uppercase(),
        _ => String::new(),
    }
}

/// Not in a session: the command doesn't exist (0.5 registers it only while live).
fn off(c: &Ctx, name: &str) -> bool {
    if c.doc().live {
        return false;
    }
    c.print(&c.tv("cmd.unknown", &[("name", name.into())]));
    true
}

fn name_of(c: &Ctx, who: u64) -> String {
    c.doc().live_info.names.get(&who).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| c.t("live.someone"))
}

/// A held area's key (live.js `areaKey`): `area:x0,y0,x1,y1@<sheet lid>`, corners sorted, rounded to 1e-3.
pub fn area_key(x0: f64, y0: f64, x1: f64, y1: f64, sp: &str) -> String {
    let r = |v: f64| cc_cad::js::num_to_string(cc_cad::js::round(v * 1000.0) / 1000.0);
    format!("area:{},{},{},{}@{sp}", r(x0.min(x1)), r(y0.min(y1)), r(x0.max(x1)), r(y0.max(y1)))
}

pub async fn livelock(c: Ctx, on: bool) -> R {
    if off(&c, if on { "LIVELOCK" } else { "LIVEUNLOCK" }) {
        return Ok(());
    }
    if c.doc().live_info.role != "host" {
        c.print(&c.t("live.lock.host_only"));
        return Ok(());
    }
    let mut ids = c.sh().host.borrow().selection();
    if ids.is_empty() {
        let options = vec![opt(&c, "L", "live.lock.opt_layer", "Layer"), opt(&c, "S", "live.lock.opt_select", "Select")];
        let k = c
            .keyword(
                c.t(if on { "live.lock.prompt" } else { "live.unlock.prompt" }),
                Opts { options, default: Some(Def::Str("S".into())), ..Opts::default() },
            )
            .await?;
        if key_of(&k) == "L" {
            let name = s_reply(&c.string(c.t("live.lock.layer"), Opts::default()).await?).trim().to_string();
            if name.is_empty() || !c.doc().layers.contains_key(&name) {
                c.print(&c.tv("live.lock.no_layer", &[("name", name)]));
                return Ok(());
            }
            let keys = vec![format!("layer:{name}")];
            c.sh().doc.borrow_mut().live_reqs.push(if on { LiveReq::Lock(keys) } else { LiveReq::Unlock(keys) });
            c.print(&c.tv(if on { "live.lock.layer_done" } else { "live.unlock.layer_done" }, &[("name", name)]));
            return Ok(());
        }
        ids = c.select(Some(c.t("live.lock.select")), true).await?.0;
    }
    if ids.is_empty() {
        return Ok(());
    }
    let keys: Vec<String> = ids.iter().take(HOLD_MAX).map(|id| format!("e:{id}")).collect();
    let n = keys.len();
    c.sh().doc.borrow_mut().live_reqs.push(if on { LiveReq::Lock(keys) } else { LiveReq::Unlock(keys) });
    c.print(&c.tv(if on { "live.lock.done" } else { "live.unlock.done" }, &[("n", n.to_string())]));
    Ok(())
}

pub async fn livehold(c: Ctx) -> R {
    if off(&c, "LIVEHOLD") {
        return Ok(());
    }
    if c.doc().live_info.role == "viewer" {
        c.print(&c.t("live.hold.viewer"));
        return Ok(());
    }
    let options = vec![
        opt(&c, "L", "live.hold.opt_layer", "Layer"),
        opt(&c, "A", "live.hold.opt_area", "Area"),
        opt(&c, "R", "live.hold.opt_release", "Release"),
    ];
    let k = key_of(&c.keyword(c.t("live.hold.prompt"), Opts { options, default: Some(Def::Str("L".into())), ..Opts::default() }).await?);
    if k == "R" {
        let mine: Vec<String> =
            c.doc().live_info.mine.iter().filter(|x| x.starts_with("layer:") || x.starts_with("area:")).cloned().collect();
        let n = mine.len();
        if n > 0 {
            c.sh().doc.borrow_mut().live_reqs.push(LiveReq::Release(mine));
        }
        c.print(&c.tv("live.hold.released", &[("n", n.to_string())]));
        return Ok(());
    }
    let (key, what) = if k == "A" {
        let p1 = c.must_point(c.t("live.hold.corner1"), Opts::default()).await?;
        let p2 = c.must_point(c.t("live.hold.corner2"), Opts { base: Some(p1), ..Opts::default() }).await?;
        if (p2.x - p1.x).abs() < 1e-9 || (p2.y - p1.y).abs() < 1e-9 {
            c.print(&c.t("live.hold.flat"));
            return Ok(());
        }
        let sp = c.doc().live_info.space_lid.clone();
        (area_key(p1.x, p1.y, p2.x, p2.y, &sp), c.t("live.hold.what_area"))
    } else {
        let cur = {
            let d = c.doc();
            if d.current.layer.is_empty() { "0".to_string() } else { d.current.layer.clone() }
        };
        let r = c
            .string(c.tv("live.hold.layer", &[("layer", cur.clone())]), Opts { default: Some(Def::Str(cur.clone())), ..Opts::default() })
            .await?;
        let typed = s_reply(&r).trim().to_string();
        let name = if typed.is_empty() { cur } else { typed };
        if !c.doc().layers.contains_key(&name) {
            c.print(&c.tv("live.lock.no_layer", &[("name", name)]));
            return Ok(());
        }
        (format!("layer:{name}"), c.tv("live.hold.what_layer", &[("layer", name)]))
    };
    let by = c.doc().live_info.held_by(&key);
    if by != 0 {
        c.print(&c.tv("live.hold.taken", &[("name", name_of(&c, by)), ("what", what)]));
        return Ok(());
    }
    {
        let mut d = c.sh().doc.borrow_mut();
        d.live_info.mine.insert(key.clone());
        d.live_reqs.push(LiveReq::Hold(vec![key]));
    }
    c.print(&c.tv("live.hold.done", &[("what", what)]));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_area_key_is_live_js_area_key() {
        assert_eq!(area_key(10.0, 20.5, 0.0, 0.0, ""), "area:0,0,10,20.5@");
        assert_eq!(area_key(1.23456, -2.0, 3.0, 4.0, "L1"), "area:1.235,-2,3,4@L1");
    }
}
