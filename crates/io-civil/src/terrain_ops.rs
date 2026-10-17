//! Script ops of terrain.rs, thin.rs, raster.rs, las.rs and geotiff_write.rs; twins of forks/tools/cvdiff/ops-crs.mjs.
//! Files travel as `{ name, b64 }`; an open raster or LAS header is kept under the call's `as` name.

use crate::crs;
use crate::crs_ops::b64;
use crate::geotiff_write::{self, DemOptions};
use crate::las::{self, Header};
use crate::raster::{self, InFile, Src};
use crate::terrain::{self, Grid};
use crate::thin::{Coverage, Thinner};
use cc_civil::js::{J, arr, numf};
use cc_civil::script::{Call, Registry, keep, obj};
use cc_civil::tin::Surface;
use serde_json::{Map, Value, json};

type R = Option<Result<Value, String>>;

fn file_of(v: Option<&Value>) -> Result<InFile, String> {
    let v = v.ok_or("no file")?;
    Ok(InFile {
        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        bytes: b64(v.get("b64").and_then(Value::as_str).unwrap_or(""))?,
    })
}
fn opt_num(v: Option<&Value>) -> Option<f64> {
    v.filter(|x| !x.is_null()).map(|x| numf(Some(x)))
}
fn gt_of(v: Option<&Value>) -> [f64; 6] {
    let a = arr(v);
    let g = |i: usize| numf(a.get(i));
    [g(0), g(1), g(2), g(3), g(4), g(5)]
}
fn grid_of(v: Option<&Value>) -> Grid {
    let g = v.cloned().unwrap_or(Value::Null);
    let nx = terrain::count_of(numf(g.get("nx")));
    let ny = terrain::count_of(numf(g.get("ny")));
    Grid {
        c0: numf(g.get("c0")),
        r0: numf(g.get("r0")),
        step: numf(g.get("step")),
        nx,
        ny,
        z: arr(g.get("z")).iter().map(|v| numf(Some(v))).collect(),
    }
}
fn b64_out(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((bytes.len().saturating_mul(4) / 3).saturating_add(4));
    for c in bytes.chunks(3) {
        let b = [c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0), c.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let ch = |s: u32| char::from(T.get(((n >> s) & 63) as usize).copied().unwrap_or(b'A'));
        out.push(ch(18));
        out.push(ch(12));
        out.push(if c.len() > 1 { ch(6) } else { '=' });
        out.push(if c.len() > 2 { ch(0) } else { '=' });
    }
    out
}
fn pts_out(f: &dyn Fn(f64, f64) -> Result<[f64; 2], String>, pts: Option<&Value>) -> Result<Value, String> {
    let mut out = Vec::new();
    for p in arr(pts) {
        let a = arr(Some(p));
        out.push(f(numf(a.first()), numf(a.get(1)))?.j());
    }
    Ok(Value::Array(out))
}
/// The transform a terrain op runs through: `{ src, pc }` → toProject(src, pc).fn.
fn fn_of(spec: Option<&Value>) -> Result<terrain::XyFn, String> {
    let s = spec.cloned().unwrap_or(Value::Null);
    let src = s.get("src").cloned().unwrap_or(Value::Null);
    let pc = s.get("pc").cloned().unwrap_or(Value::Null);
    Ok(terrain::to_project(&src, &pc)?.0)
}
fn classes_of(v: Option<&Value>) -> Option<Vec<f64>> {
    v.filter(|c| c.is_array()).map(|c| arr(Some(c)).iter().map(|x| numf(Some(x))).collect())
}

/// A TIFF's tags and inflated blocks (what the harness compares of a DEFLATE file: the zlib bytes themselves differ).
pub fn tiff_view(b: &[u8]) -> Value {
    let rd16 = |o: usize| {
        b.get(o..o.saturating_add(2)).map_or(0, |s| u16::from_le_bytes([s.first().copied().unwrap_or(0), s.get(1).copied().unwrap_or(0)]))
    };
    let rd32 = |o: usize| {
        b.get(o..o.saturating_add(4)).map_or(0, |s| {
            u32::from_le_bytes([
                s.first().copied().unwrap_or(0),
                s.get(1).copied().unwrap_or(0),
                s.get(2).copied().unwrap_or(0),
                s.get(3).copied().unwrap_or(0),
            ])
        })
    };
    let n = rd16(8) as usize;
    let mut tags = Vec::new();
    let (mut offs, mut cnts) = (Vec::new(), Vec::new());
    for i in 0..n.min(4096) {
        let e = 10usize.saturating_add(i.saturating_mul(12));
        let (tag, ty, count) = (rd16(e), rd16(e.saturating_add(2)), rd32(e.saturating_add(4)));
        let unit: usize = match ty {
            3 => 2,
            4 => 4,
            12 => 8,
            _ => 1,
        };
        let size = unit.saturating_mul(count as usize);
        let at = if size <= 4 { e.saturating_add(8) } else { rd32(e.saturating_add(8)) as usize };
        let data = b.get(at..at.saturating_add(size)).unwrap_or(&[]);
        if matches!(tag, 273 | 324) {
            offs = data
                .chunks(4)
                .map(|c| {
                    u32::from_le_bytes([
                        c.first().copied().unwrap_or(0),
                        c.get(1).copied().unwrap_or(0),
                        c.get(2).copied().unwrap_or(0),
                        c.get(3).copied().unwrap_or(0),
                    ]) as usize
                })
                .collect();
            tags.push(json!({ "tag": tag, "type": ty, "count": count }));
        } else if matches!(tag, 279 | 325) {
            cnts = data
                .chunks(4)
                .map(|c| {
                    u32::from_le_bytes([
                        c.first().copied().unwrap_or(0),
                        c.get(1).copied().unwrap_or(0),
                        c.get(2).copied().unwrap_or(0),
                        c.get(3).copied().unwrap_or(0),
                    ]) as usize
                })
                .collect();
            tags.push(json!({ "tag": tag, "type": ty, "count": count }));
        } else {
            tags.push(json!({ "tag": tag, "type": ty, "count": count, "data": b64_out(data) }));
        }
    }
    let blocks: Vec<Value> = offs
        .iter()
        .zip(cnts.iter())
        .map(|(o, c)| {
            let raw = b.get(*o..o.saturating_add(*c)).unwrap_or(&[]);
            match miniz_oxide::inflate::decompress_to_vec_zlib(raw) {
                Ok(d) => Value::String(b64_out(&d)),
                Err(_) => Value::String(b64_out(raw)),
            }
        })
        .collect();
    json!({ "tags": tags, "blocks": blocks })
}

pub fn handle(reg: &mut Registry, c: &Call) -> R {
    let r: Result<Value, String> = match c.op {
        "terrain.unitFromName" => Ok(terrain::unit_from_name(&c.raw(0).filter(|v| !v.is_null()).map(cc_civil::js::to_js_string).unwrap_or_default()).j_str()),
        "terrain.crsFromGeoKeys" => Ok(terrain::crs_from_geo_keys(c.raw(0).and_then(Value::as_object)).j()),
        "terrain.projectZUnit" => Ok(Value::String(terrain::project_z_unit(&c.raw(0).cloned().unwrap_or(Value::Null)).into())),
        "terrain.zFactor" => Ok(terrain::z_factor(c.str(0), c.str(1)).j()),
        "terrain.unitWords" => Ok(json!({ "m": terrain::unit_words("m"), "ft": terrain::unit_words("ft"), "ftUS": terrain::unit_words("ftUS"), "short": [terrain::unit_short("m"), terrain::unit_short("ft"), terrain::unit_short("ftUS")], "epsg": [terrain::epsg_of_unit("m").j(), terrain::epsg_of_unit("ft").j(), terrain::epsg_of_unit("ftUS").j()] })),
        "terrain.toProject" => {
            let src = c.raw(0).cloned().unwrap_or(Value::Null);
            let pc = c.raw(1).cloned().unwrap_or(Value::Null);
            terrain::to_project(&src, &pc).and_then(|(f, id)| Ok(json!({ "identity": id, "pts": pts_out(&*f, c.raw(2))? })))
        }
        "terrain.toSource" => {
            let src = c.raw(0).cloned().unwrap_or(Value::Null);
            let pc = c.raw(1).cloned().unwrap_or(Value::Null);
            terrain::to_source(&src, &pc).and_then(|f| pts_out(&*f, c.raw(2)))
        }
        "terrain.lonLatOf" => {
            let src = c.raw(0).cloned().unwrap_or(Value::Null);
            terrain::lon_lat_of(&src).and_then(|f| match f {
                None => Ok(Value::Null),
                Some(f) => pts_out(&*f, c.raw(1)),
            })
        }
        "terrain.boxThrough" => {
            let b = terrain::read_box(c.raw(0));
            fn_of(c.raw(1)).and_then(|f| match b {
                Some(b) => terrain::box_through(&b, &*f, opt_num(c.raw(2)).unwrap_or(16.0)).map(|o| o.map_or(Value::Null, |x| x.j())),
                None => Err("Cannot read properties of null (reading 'minX')".into()),
            })
        }
        "terrain.gridWindow" => {
            let s = c.raw(0).cloned().unwrap_or(Value::Null);
            Ok(terrain::grid_window(&gt_of(s.get("gt")), numf(s.get("width")), numf(s.get("height")), terrain::read_box(c.raw(1)).as_ref()).map_or(Value::Null, |w| w.j()))
        }
        "terrain.nodeXY" => {
            let s = c.raw(0).cloned().unwrap_or(Value::Null);
            Ok(terrain::node_xy(&gt_of(s.get("gt")), c.num(1), c.num(2)).j())
        }
        "terrain.nextNumber" => Ok(terrain::next_number(arr(c.raw(0))).j()),
        "terrain.gridPoints" => {
            let o = c.raw(2).cloned().unwrap_or(Value::Null);
            let s = c.raw(1).cloned().unwrap_or(Value::Null);
            fn_of(o.get("fn")).and_then(|f| {
                let zf = opt_num(o.get("zf")).unwrap_or(1.0);
                let desc = o.get("desc").and_then(Value::as_str).unwrap_or("DEM");
                let start = opt_num(o.get("start")).unwrap_or(1.0);
                terrain::grid_points(&grid_of(c.raw(0)), &gt_of(s.get("gt")), &*f, zf, desc, start).map(|p| p.j())
            })
        }
        "terrain.gridTin" => {
            let o = c.raw(2).cloned().unwrap_or(Value::Null);
            let s = c.raw(1).cloned().unwrap_or(Value::Null);
            fn_of(o.get("fn")).and_then(|f| terrain::grid_tin(&grid_of(c.raw(0)), &gt_of(s.get("gt")), &*f, opt_num(o.get("zf")).unwrap_or(1.0)).map(|t| t.j()))
        }
        "terrain.niceStep" => Ok(terrain::nice_step(c.num(0)).j()),

        "thin.run" => {
            let o = c.raw(0).cloned().unwrap_or(Value::Null);
            let origin = arr(o.get("origin"));
            let region = terrain::read_box(o.get("region"));
            let mode = o.get("mode").and_then(Value::as_str).unwrap_or("nearest");
            Thinner::new(
                numf(o.get("cell")),
                mode,
                if o.get("origin").is_some_and(|v| !v.is_null()) { [numf(origin.first()), numf(origin.get(1))] } else { [0.0, 0.0] },
                classes_of(o.get("classes")).as_deref(),
                region,
                opt_num(o.get("max")).unwrap_or(f64::INFINITY),
            )
            .map(|mut th| {
                let adds: Vec<bool> = arr(c.raw(1))
                    .iter()
                    .map(|p| {
                        let a = arr(Some(p));
                        th.add(numf(a.first()), numf(a.get(1)), numf(a.get(2)), numf(a.get(3)))
                    })
                    .collect();
                let pts: Vec<Value> = th.points().iter().map(|p| json!({ "x": p[0].j(), "y": p[1].j(), "z": p[2].j() })).collect();
                json!({ "adds": adds, "state": th.j(), "points": pts })
            })
        }
        "thin.gridStep" => Ok(crate::thin::grid_step(c.num(0), c.num(1)).j()),
        "thin.coverage" => match terrain::read_box(c.raw(0)) {
            None => Err("Cannot read properties of undefined (reading 'maxX')".into()),
            Some(b) => {
                let n = terrain::count_of(opt_num(c.raw(1)).unwrap_or(128.0));
                let mut cv = Coverage::new(b, n.clamp(1, 4096));
                for p in arr(c.raw(2)) {
                    let a = arr(Some(p));
                    cv.add(numf(a.first()), numf(a.get(1)), numf(a.get(2)));
                }
                let est: Vec<Value> = arr(c.raw(3))
                    .iter()
                    .map(|e| {
                        let (p, i) = cv.estimate(numf(e.get("spacing")), classes_of(e.get("classes")).as_deref(), terrain::read_box(e.get("box")).as_ref());
                        json!({ "points": p.j(), "inArea": i.j() })
                    })
                    .collect();
                let counts: Vec<Value> = cv.counts().iter().map(|(k, v)| json!([k.j(), v.j()])).collect();
                Ok(json!({ "total": cv.total.j(), "w": cv.w.j(), "h": cv.h.j(), "counts": counts, "estimates": est }))
            }
        },

        "raster.probeTiff" => file_of(c.raw(0)).and_then(|f| raster::probe_tiff(&f.bytes)),
        "raster.open" => {
            let files: Result<Vec<InFile>, String> = arr(c.raw(0)).iter().map(|f| file_of(Some(f))).collect();
            files.and_then(|fs| raster::open_raster(&fs)).map(|s| {
                let v = s.j();
                keep(reg, c, s);
                v
            })
        }
        "raster.readWindow" => {
            let o = c.raw(0).cloned().unwrap_or(Value::Null);
            obj::<Src>(reg, c.this, "raster").and_then(|s| {
                raster::read_window(s, opt_num(o.get("c0")), opt_num(o.get("r0")), opt_num(o.get("c1")), opt_num(o.get("r1")), opt_num(o.get("step"))).map(|g| g.j())
            })
        }
        "raster.sampleStats" => match c.this.and_then(|n| reg.get_mut::<Src>(n)) {
            Some(s) => raster::sample_stats(s).map(|st| st.j()),
            None => Err(format!("no object {}", c.this.unwrap_or("undefined"))),
        },
        "raster.readGeoImage" => file_of(c.raw(0)).and_then(|f| raster::read_geo_image(&f.name, &f.bytes)).map(|g| {
            json!({ "width": g.width.j(), "height": g.height.j(), "rgba": b64_out(&g.rgba), "affine": g.affine.to_vec().j(), "bbox": g.bbox.to_vec().j(), "crs": g.crs, "code": g.code.j(), "crsNote": g.crs_note, "notes": g.notes })
        }),
        "raster.fmtBig" => Ok(Value::String(raster::fmt_big(c.num(0)))),

        "las.readHeader" => file_of(c.raw(0)).and_then(|f| {
            las::read_las_header(&f).map(|h| {
                let v = h.j();
                keep(reg, c, (f, h));
                v
            })
        }),
        "las.scan" => {
            let max = opt_num(c.raw(0)).unwrap_or(f64::INFINITY);
            match c.this.and_then(|n| reg.get_mut::<(InFile, Header)>(n)) {
                None => Err(format!("no object {}", c.this.unwrap_or("undefined"))),
                Some((f, h)) => {
                    let mut pts: Vec<Value> = Vec::new();
                    let mut sum = [0.0f64; 4];
                    let mut seen = 0.0;
                    let read = las::scan_points(f, h, &mut |x, y, z, cls| {
                        if seen < max {
                            pts.push(json!([x.j(), y.j(), z.j(), cls.j()]));
                        }
                        seen += 1.0;
                        sum[0] += x;
                        sum[1] += y;
                        sum[2] += z;
                        sum[3] += cls;
                    });
                    read.map(|r| json!({ "read": r.j(), "pts": pts, "sum": sum.to_vec().j(), "notes": h.notes }))
                }
            }
        }
        "las.className" => Ok(las::class_name(c.num(0)).map_or(Value::Null, Value::String)),

        "gtw.averageEdge" => obj::<Surface>(reg, c.this, "surface").map(|s| geotiff_write::average_edge(s).j()),
        "gtw.surfaceGrid" => {
            let o = c.raw(0).cloned().unwrap_or(Value::Null);
            let frame = match o.get("crs").filter(|v| cc_civil::js::truthy(Some(v))) {
                Some(cv) => crs::frame(cv),
                None => Ok(None),
            };
            frame.and_then(|fr| {
                let s = obj::<Surface>(reg, c.this, "surface")?;
                let tg = |x: f64, y: f64| fr.as_ref().map_or([x, y], |f| f.to_grid(x, y));
                let fg = |x: f64, y: f64| fr.as_ref().map_or([x, y], |f| f.from_grid(x, y));
                geotiff_write::surface_grid(s, numf(o.get("cell")), &tg, &fg, opt_num(o.get("maxCells")).unwrap_or(25e6)).map(|g| {
                    let vals: Vec<Value> = g.values.iter().map(|v| f64::from(*v).j()).collect();
                    json!({ "width": g.width.j(), "height": g.height.j(), "gt": g.gt.to_vec().j(), "values": vals })
                })
            })
        }
        "gtw.write" => {
            let o = c.raw(0).cloned().unwrap_or(Value::Null);
            let vals: Vec<f32> = arr(o.get("values")).iter().map(|v| f32_of(numf(Some(v)))).collect();
            let opts = DemOptions {
                epsg: opt_num(o.get("epsg")),
                citation: o.get("citation").and_then(Value::as_str).unwrap_or("").to_string(),
                xy_unit: o.get("xyUnit").and_then(Value::as_str).unwrap_or("m").to_string(),
                z_unit: o.get("zUnit").and_then(Value::as_str).unwrap_or("m").to_string(),
                description: o.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                tiled: o.get("tiled").and_then(Value::as_bool).unwrap_or(true),
                tile: opt_num(o.get("tile")).unwrap_or(256.0),
                deflate: o.get("compress").and_then(Value::as_str).unwrap_or("deflate") == "deflate",
            };
            geotiff_write::write_dem_tiff(numf(o.get("width")), numf(o.get("height")), gt_of(o.get("gt")), &vals, &opts).map(|b| {
                let mut m = Map::new();
                if opts.deflate {
                    m.insert("view".into(), tiff_view(&b));
                } else {
                    m.insert("bytes".into(), Value::String(b64_out(&b)));
                }
                // read back through our own reader
                let back = raster::open_raster(&[InFile { name: "out.tif".into(), bytes: b }]).and_then(|s| raster::read_window(&s, None, None, None, None, None).map(|g| (s.j(), g.j())));
                m.insert("back".into(), match back {
                    Ok((mut s, g)) => {
                        // DEFLATE: the zlib bytes (so the file size) are the encoder's own
                        if opts.deflate
                            && let Some(o) = s.as_object_mut()
                        {
                            o.remove("size");
                        }
                        json!({ "src": s, "grid": g })
                    }
                    Err(e) => json!({ "err": e }),
                });
                Value::Object(m)
            })
        }
        _ => return None,
    };
    Some(r)
}

/// `Math.fround` into an f32.
#[allow(clippy::cast_possible_truncation)]
fn f32_of(v: f64) -> f32 {
    v as f32
}

trait JStr {
    fn j_str(&self) -> Value;
}
impl JStr for Option<&'static str> {
    fn j_str(&self) -> Value {
        self.map_or(Value::Null, |s| Value::String(s.into()))
    }
}
