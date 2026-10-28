//! Pictures as map layers, the rest of gis-map-ui.js addImageFiles: a GeoTIFF picture (RGB, palette or 8-bit; cc-io-civil
//! raster.rs readGeoImage, its caps kept) and the question 0.5 asks when a picture comes without a .prj (crsForFile:
//! the project's system or its own coordinates, or one picked).

use cc_civil::js::{string_of, to_fixed};
use cc_civil::tr;
use serde_json::Value;

use crate::civilcrs::{Ask, AskKind, PickFor};
use crate::civilview::{CivilView, t0};

/// A picture kept in a project can be up to 25 MB (gis-map-ui.js B.MAX_IMAGE_BYTES).
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;

/// A picture waiting for its system.
#[derive(Clone, Debug)]
pub struct PicWait {
    pub name: String,
    /// the data: URL kept in the project
    pub data: String,
    pub w: f64,
    pub h: f64,
    pub gt: [f64; 6],
    /// a GeoTIFF (its own words when added)
    pub tif: bool,
    pub notes: Vec<String>,
}

impl CivilView {
    /// `crsForFile(prj, name)`: the .prj's system, else asked.
    pub fn pic_crs(&mut self, pw: PicWait, prj: Option<(String, String)>) {
        if let Some((pn, text)) = prj {
            crate::civilgis::ensure_catalog();
            match cc_io_civil::crs::crs_from_prj(&text) {
                Ok(r) => {
                    self.pic_finish(pw, Some(r.crs));
                    return;
                }
                Err(e) => self.message = tr!("mapl.prj.bad", name = pn.as_str(), error = e),
            }
        }
        let pc = self.pcrs();
        let name = pw.name.clone();
        self.gis3.pic_wait = Some(pw);
        self.gis3.ask = Some(Ask {
            kind: AskKind::Other("mapcrs".into()),
            title: tr!("mapl.where.title", name = name.as_str()),
            text: match &pc {
                Some(c) => tr!("mapl.where.text_pc", crs = string_of(c.get("name"))),
                None => t0("mapl.where.text_nopc"),
            },
            buttons: vec![
                ("cancel", t0("cad.ui.cancel")),
                ("pick", t0("mapl.where.pick")),
                ("same", t0(if pc.is_some() { "mapl.where.project_sys" } else { "mapl.where.project_coords" })),
            ],
        });
    }

    /// The answer to "Where is …?".
    pub fn pic_ask(&mut self, b: &str) {
        match b {
            "same" => {
                let Some(pw) = self.gis3.pic_wait.take() else { return };
                let c = self.pcrs().map(|mut c| {
                    if let Some(o) = c.as_object_mut() {
                        o.insert("local".into(), Value::Null);
                    }
                    c
                });
                self.pic_finish(pw, c);
            }
            "pick" => {
                let name = self.gis3.pic_wait.as_ref().map(|p| p.name.clone()).unwrap_or_default();
                self.crs_dlg = None;
                self.crs_open(Some((tr!("mapl.where.reason", name = name.as_str()), None, "source", PickFor::MapPicture)));
            }
            _ => self.gis3.pic_wait = None,
        }
    }

    /// The picked system for the waiting picture (None: cancelled, nothing added).
    pub fn maps_crs_answer(&mut self, v: Option<Value>) {
        let Some(pw) = self.gis3.pic_wait.take() else { return };
        if let Some(c) = v {
            self.pic_finish(pw, Some(c));
        }
    }

    /// The picture becomes a map layer (one undo step) and is kept in the project.
    pub fn pic_finish(&mut self, pw: PicWait, crs: Option<Value>) {
        let used: Vec<String> = self
            .project
            .gis_images
            .iter()
            .map(|(k, _)| k.clone())
            .chain(self.maps_list().iter().map(|m| string_of(m.get("image"))))
            .collect();
        let mut n = 1;
        while used.contains(&format!("img{n}")) {
            n += 1;
        }
        let img_id = format!("img{n}");
        self.project.gis_images.push((img_id.clone(), pw.data.clone()));
        let stem = pw.name.rsplit_once('.').map_or(pw.name.as_str(), |(a, _)| a).to_string();
        let id = self.new_map_id();
        let rec = crate::civilmaps::record(
            &id,
            "image",
            &stem,
            "",
            "",
            "",
            "image/png",
            "1.3.0",
            crs.as_ref(),
            "",
            &img_id,
            (pw.w, pw.h),
            Some(pw.gt),
            "",
            "",
            "",
        );
        self.add_map(rec, &tr!("mapl.undo.picture", name = pw.name.as_str()));
        let cname = crs.as_ref().map(|c| string_of(c.get("name")));
        if pw.tif {
            let mut m = match &cname {
                Some(c) => tr!("mapl.added.tif_crs", name = pw.name.as_str(), w = pw.w, h = pw.h, crs = c.as_str()),
                None => tr!("mapl.added.tif", name = pw.name.as_str(), w = pw.w, h = pw.h),
            };
            if !pw.notes.is_empty() {
                m.push(' ');
                m.push_str(&pw.notes.join(" "));
            }
            self.message = m;
        } else {
            self.message = match &cname {
                Some(c) => tr!("mapl.added.pic_crs", name = pw.name.as_str(), w = pw.w, h = pw.h, crs = c.as_str()),
                None => tr!("mapl.added.pic_proj", name = pw.name.as_str(), w = pw.w, h = pw.h),
            };
            if let Some(c) = &cname
                && self.pcrs().is_none()
            {
                self.message = tr!("mapl.added.pic_wait", name = pw.name.as_str(), crs = c.as_str());
            }
        }
        self.fit();
    }

    /// An RGB GeoTIFF (addGeoTiff): its pixels as a PNG (any transparency) or a JPEG kept in the project; a world file
    /// given with it wins over what the TIFF says; no system in it (or a .prj with it): asked as for any picture.
    pub fn pic_tiff(&mut self, name: &str, bytes: &[u8], world: Option<&[u8]>, prj: Option<(String, String)>) {
        crate::civilgis::ensure_catalog();
        let g = match cc_io_civil::raster::read_geo_image(name, bytes) {
            Ok(g) => g,
            Err(e) => {
                self.message = e;
                return;
            }
        };
        let mut gt = g.affine;
        if let Some(w) = world {
            match cc_io_civil::basemap::parse_world_file(&String::from_utf8_lossy(w)) {
                Ok(x) => gt = x,
                Err(e) => {
                    self.message = e.0;
                    return;
                }
            }
        }
        let (w, h) = (g.width, g.height);
        let alpha = g.rgba.chunks(4).any(|p| p.get(3).is_some_and(|a| *a < 255));
        let data = match encode_rgba(&g.rgba, w, h, alpha) {
            Ok(d) => d,
            Err(e) => {
                self.message = e;
                return;
            }
        };
        if data.len() > MAX_IMAGE_BYTES {
            self.message = tr!("mapl.tif.big", name = name, mb = to_fixed(data.len() as f64 / 1_048_576.0, 1));
            return;
        }
        let mime = if alpha { "image/png" } else { "image/jpeg" };
        let url = format!("data:{mime};base64,{}", crate::civilmaps::b64_encode(&data));
        let pw = PicWait { name: name.to_string(), data: url, w, h, gt, tif: true, notes: g.notes.clone() };
        let own = cc_civil::js::truthy(Some(&g.crs)).then(|| g.crs.clone());
        match (prj, own) {
            (None, Some(c)) => self.pic_finish(pw, Some(c)),
            (prj, _) => self.pic_crs(pw, prj),
        }
    }
}

/// RGBA pixels → PNG (with transparency) or JPEG (quality 90), as the page's canvas.convertToBlob.
fn encode_rgba(rgba: &[u8], w: f64, h: f64, alpha: bool) -> Result<Vec<u8>, String> {
    let (Some(w), Some(h)) = (cc_civil::js::usize_of(w), cc_civil::js::usize_of(h)) else { return Err("bad picture size".into()) };
    let (Ok(w), Ok(h)) = (u32::try_from(w), u32::try_from(h)) else { return Err("bad picture size".into()) };
    let mut out = Vec::new();
    if alpha {
        let img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or("bad picture size")?;
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    } else {
        let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| p.iter().take(3).copied()).collect();
        let img = image::RgbImage::from_raw(w, h, rgb).ok_or("bad picture size")?;
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        enc.encode_image(&img).map_err(|e| e.to_string())?;
    }
    Ok(out)
}
