//! cloud-cad.js's commands: POINTCLOUDATTACH (PCATTACH) puts a LAS / LAZ / XYZ / PTS point cloud under the drawing —
//! at the file's own coordinates (Enter) or with its lower-left corner at a point — and POINTCLOUD (PC) sets it up:
//! Slice (a plan cut to trace from), Height, Thickness, Color, Size, Visible, Zoom, Detach. The cloud is the host's
//! ([`crate::Host::cloud`]): not in the drawing, not in undo.

use cc_cad::Doc;
use cc_cad::js;
use cc_cad_geom::geom::Bounds;
use cc_geom::P2;
use cc_io_civil::pointcloud::{self as pcl, Cloud};
use cc_io_civil::raster::{InFile, locale_int};

use crate::cloud::COLOR_KEYS;
use crate::engine::{Ctx, Opts, PickedFile, R, Reply, Shared};
use crate::input::Def;

/// `doc.meta.units || ''`
pub fn units_of(doc: &Doc) -> String {
    doc.meta.get("units").filter(|v| js::truthy(Some(v))).map(js::to_js_string).unwrap_or_default()
}

/// `String(Math.round(v * 1000) / 1000)`
fn fmt3(v: f64) -> String {
    js::num_to_string(js::round(v * 1000.0) / 1000.0)
}

/// The points read: `readCloudInfo`, then `loadCloud` (budget 2 million); the time the load started (ms).
fn read(sh: &Shared, file: PickedFile) -> Result<(Cloud, Option<f64>), String> {
    sh.print(&sh.tv("pc.reading", &[("file", file.name.clone())]));
    let tr = cc_cad::i18n::catalog();
    let f = InFile { name: file.name, bytes: file.bytes };
    let info = pcl::read_cloud_info(&tr, &f)?;
    let t0 = sh.host.borrow().now_ms();
    let cloud = pcl::load_cloud(&tr, &f, &info, 2e6)?;
    Ok((cloud, t0))
}

/// The second half of `attachFile`: attach, then say what shows.
fn finish(sh: &Shared, name: &str, cloud: Cloud, insert: Option<P2>, t0: Option<f64>) {
    let Some(pc) = sh.host.borrow().cloud() else { return };
    let units = units_of(&sh.doc.borrow());
    let notes = cloud.notes.clone();
    let has_unit = cloud.unit.is_some();
    let (kept, read) = (cloud.n, cloud.read);
    pc.borrow_mut().attach(cloud, insert);
    let secs = match (t0, sh.host.borrow().now_ms()) {
        (Some(a), Some(b)) => (b - a) / 1000.0,
        _ => 0.0,
    };
    sh.print(&sh.tv(
        "pc.opened",
        &[("file", name.to_string()), ("kept", locale_int(kept as f64)), ("read", locale_int(read)), ("s", js::to_fixed(secs, 1))],
    ));
    for n in &notes {
        sh.print(&format!("  {n}"));
    }
    let (on, h) = {
        let p = pc.borrow();
        (p.slice.on, p.slice_spec(&units).0)
    };
    if on {
        sh.print(&format!("  {}", sh.tv("pc.sliced", &[("h", fmt3(h))])));
    }
    if units.is_empty() || !has_unit {
        sh.print(&format!("  {}", sh.t(if has_unit { "pc.units.drawing" } else { "pc.units.cloud" })));
    }
    sh.print(&sh.t("pc.next"));
    zoom(sh);
}

/// `host.zoomCloud()`
fn zoom(sh: &Shared) {
    let Some(pc) = sh.host.borrow().cloud() else { return };
    let units = units_of(&sh.doc.borrow());
    let b = pc.borrow().bbox(&units);
    if let Some([x0, y0, x1, y1]) = b {
        sh.host.borrow_mut().zoom_cloud(Bounds { min_x: x0, min_y: y0, max_x: x1, max_y: y1 });
    }
}

/// `attachFile({ host, print, doc }, file)` from the window's Open (or a dropped file): at its own coordinates.
pub fn attach_now(sh: &Shared, file: PickedFile) -> bool {
    if sh.host.borrow().cloud().is_none() {
        sh.print(&sh.t("pc.nobrowser"));
        return false;
    }
    let name = file.name.clone();
    match read(sh, file) {
        Ok((cloud, t0)) => {
            finish(sh, &name, cloud, None, t0);
            true
        }
        Err(why) => {
            sh.print(&sh.tv("pc.failed", &[("file", name), ("why", why)]));
            true
        }
    }
}

pub async fn pointcloudattach(c: Ctx) -> R {
    let picked = c.pick_cloud(c.t("files.type.cloud")).await?;
    let Some(picked) = picked else {
        c.print(&c.t("pc.nobrowser"));
        return Ok(());
    };
    let Some(file) = picked else { return Ok(()) };
    if c.sh().host.borrow().cloud().is_none() {
        c.print(&c.t("pc.nobrowser"));
        return Ok(());
    }
    let name = file.name.clone();
    let (cloud, t0) = match read(c.sh(), file) {
        Ok(x) => x,
        Err(why) => {
            c.print(&c.tv("pc.failed", &[("file", name), ("why", why)]));
            return Ok(());
        }
    };
    let p = c.point(c.t("pc.prompt.insert"), Opts { options: vec![c.opt("F", "File coordinates")], none: true, ..Opts::default() }).await?;
    finish(c.sh(), &name, cloud, p.point(), t0);
    Ok(())
}

pub async fn pointcloud(c: Ctx) -> R {
    let pc = c.sh().host.borrow().cloud();
    let Some(pc) = pc.filter(|p| p.borrow().has()) else {
        c.print(&c.t("pc.none"));
        return Ok(());
    };
    loop {
        let units = units_of(&c.doc());
        let (sh_h, sh_t) = pc.borrow().slice_spec(&units);
        let (file, kept, on, color) = {
            let p = pc.borrow();
            let Some(cl) = p.cloud.as_deref() else { return Ok(()) };
            (cl.name.clone(), cl.n, p.slice.on, p.color)
        };
        let slice = if on { c.tv("pc.state.slice", &[("h", fmt3(sh_h)), ("thick", fmt3(sh_t))]) } else { c.t("pc.state.all") };
        c.print(&c.tv(
            "pc.state",
            &[("file", file), ("kept", locale_int(kept as f64)), ("slice", slice), ("color", c.t(&format!("pc.color.{color}")))],
        ));
        let options = vec![
            c.opt("S", "Slice"),
            c.opt("H", "Height"),
            c.opt("T", "Thickness"),
            c.opt("C", "Color"),
            c.opt("SI", "Size"),
            c.opt("V", "Visible"),
            c.opt("Z", "Zoom"),
            c.opt("D", "Detach"),
        ];
        let r = c.keyword(c.t("pc.prompt.option"), Opts { options, ..Opts::default() }).await?;
        let k = match r {
            Reply::Opt(k) | Reply::Str(k) => k,
            _ => return Ok(()),
        };
        match k.as_str() {
            "" => return Ok(()),
            "S" => {
                let on = {
                    let mut p = pc.borrow_mut();
                    p.slice.on = !p.slice.on;
                    p.changed();
                    p.slice.on
                };
                c.print(&c.t(if on { "pc.slice.on.cmd" } else { "pc.slice.off.cmd" }));
            }
            "H" => {
                let v = c.number(c.t("pc.prompt.height"), Opts { default: Some(Def::Num(sh_h)), length: true, ..Opts::default() }).await?;
                if let Reply::Num(v) = v
                    && v.is_finite()
                {
                    let mut p = pc.borrow_mut();
                    p.slice.h = Some(v);
                    p.slice.on = true;
                    p.changed();
                }
            }
            "T" => {
                let v = c.number(c.t("pc.prompt.thick"), Opts { default: Some(Def::Num(sh_t)), length: true, ..Opts::default() }).await?;
                match v {
                    Reply::Num(v) if v.is_finite() && v > 0.0 => {
                        let mut p = pc.borrow_mut();
                        p.slice.thick = Some(v);
                        p.changed();
                    }
                    _ => c.print(&c.t("pc.thick.bad")),
                }
            }
            "C" => {
                let (modes, cur) = {
                    let p = pc.borrow();
                    let Some(cl) = p.cloud.as_deref() else { return Ok(()) };
                    (pcl::color_modes(cl), p.color)
                };
                let options = COLOR_KEYS
                    .iter()
                    .filter(|(_, m)| modes.contains(m))
                    .map(|(key, m)| {
                        let label = match *m {
                            "rgb" => "File",
                            "height" => "Height",
                            "intensity" => "Intensity",
                            "class" => "Class",
                            _ => "One",
                        };
                        c.opt(key, label)
                    })
                    .collect();
                let def = COLOR_KEYS.iter().find(|(_, m)| *m == cur).map(|(k, _)| Def::Str((*k).to_string()));
                let cr = c.keyword(c.t("pc.prompt.color"), Opts { options, default: def, ..Opts::default() }).await?;
                let ck = match cr {
                    Reply::Opt(k) | Reply::Str(k) => k,
                    _ => String::new(),
                };
                if let Some((_, m)) = COLOR_KEYS.iter().find(|(key, _)| *key == ck) {
                    pc.borrow_mut().set_color(m, [120, 120, 120]);
                }
            }
            "SI" => {
                let size = pc.borrow().size;
                let v = c.number(c.t("pc.prompt.size"), Opts { default: Some(Def::Num(size)), ..Opts::default() }).await?;
                match v {
                    Reply::Num(v) if (1.0..=8.0).contains(&v) => {
                        let mut p = pc.borrow_mut();
                        p.size = js::round(v);
                        p.changed();
                    }
                    _ => c.print(&c.t("pc.size.bad")),
                }
            }
            "V" => {
                let mut p = pc.borrow_mut();
                p.visible = !p.visible;
                p.changed();
            }
            "Z" => zoom(c.sh()),
            "D" => {
                let name = pc.borrow().cloud.as_deref().map(|c| c.name.clone()).unwrap_or_default();
                pc.borrow_mut().detach();
                c.print(&c.tv("pc.detached", &[("file", name)]));
                return Ok(());
            }
            _ => {}
        }
    }
}
