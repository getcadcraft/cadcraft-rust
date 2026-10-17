//! proj4js `lib/projections/*.js`, one for one: `init` (on the projection object) and `forward` / `inverse` (radians
//! ↔ metres on the projection's own ellipsoid). Every projection the CivilCraft catalog uses is here, plus the small
//! ones; vandg, ortho, qsc, robin, geocent, tpers, geos, bonne and ob_tran are registered by name but refuse to build
//! ("not ported") — no catalog system uses them.

use crate::common::*;
use crate::math::{abs, acos, asin, atan, atan2, cos, exp, log, pow, sin, sqrt, tan};
use crate::proj::{Out, Proj, Pt, n, truthy};
use crate::val::Val;

type R<T> = Result<T, String>;

/// Projection ids in registration order (projections.js: merc, longlat; projs.js: the rest), with their names.
pub static REGISTRY: &[(&str, &[&str])] = &[
    (
        "merc",
        &["Mercator", "Popular Visualisation Pseudo Mercator", "Mercator_1SP", "Mercator_Auxiliary_Sphere", "Mercator_Variant_A", "merc"],
    ),
    ("longlat", &["longlat", "identity", "lonlat", "latlon", "latlong"]),
    ("tmerc", &["Fast_Transverse_Mercator", "Fast Transverse Mercator"]),
    (
        "etmerc",
        &[
            "Extended_Transverse_Mercator",
            "Extended Transverse Mercator",
            "etmerc",
            "Transverse_Mercator",
            "Transverse Mercator",
            "Gauss Kruger",
            "Gauss_Kruger",
            "tmerc",
        ],
    ),
    ("utm", &["Universal Transverse Mercator System", "utm"]),
    (
        "sterea",
        &["Stereographic_North_Pole", "Oblique_Stereographic", "sterea", "Oblique Stereographic Alternative", "Double_Stereographic"],
    ),
    (
        "stere",
        &["stere", "Stereographic_South_Pole", "Polar_Stereographic_variant_A", "Polar_Stereographic_variant_B", "Polar_Stereographic"],
    ),
    ("somerc", &["somerc"]),
    (
        "omerc",
        &[
            "Hotine_Oblique_Mercator",
            "Hotine Oblique Mercator",
            "Hotine_Oblique_Mercator_variant_A",
            "Hotine_Oblique_Mercator_Variant_B",
            "Hotine_Oblique_Mercator_Azimuth_Natural_Origin",
            "Hotine_Oblique_Mercator_Two_Point_Natural_Origin",
            "Hotine_Oblique_Mercator_Azimuth_Center",
            "Oblique_Mercator",
            "omerc",
        ],
    ),
    (
        "lcc",
        &[
            "Lambert Tangential Conformal Conic Projection",
            "Lambert_Conformal_Conic",
            "Lambert_Conformal_Conic_1SP",
            "Lambert_Conformal_Conic_2SP",
            "lcc",
            "Lambert Conic Conformal (1SP)",
            "Lambert Conic Conformal (2SP)",
        ],
    ),
    ("krovak", &["Krovak", "Krovak Modified", "Krovak (North Orientated)", "Krovak Modified (North Orientated)", "krovak"]),
    ("cass", &["Cassini", "Cassini_Soldner", "cass"]),
    ("laea", &["Lambert Azimuthal Equal Area", "Lambert_Azimuthal_Equal_Area", "laea"]),
    ("aea", &["Albers_Conic_Equal_Area", "Albers_Equal_Area", "Albers", "aea"]),
    ("gnom", &["gnom"]),
    ("cea", &["cea"]),
    ("eqc", &["Equirectangular", "Equidistant_Cylindrical", "Equidistant_Cylindrical_Spherical", "eqc"]),
    ("poly", &["Polyconic", "American_Polyconic", "poly"]),
    ("nzmg", &["New_Zealand_Map_Grid", "nzmg"]),
    ("mill", &["Miller_Cylindrical", "mill"]),
    ("sinu", &["Sinusoidal", "sinu"]),
    ("eck6", &["Eckert_VI", "eck6"]),
    ("moll", &["Mollweide", "moll"]),
    ("eqdc", &["Equidistant_Conic", "eqdc"]),
    ("vandg", &["Van_der_Grinten_I", "VanDerGrinten", "Van_der_Grinten", "vandg"]),
    ("aeqd", &["Azimuthal_Equidistant", "aeqd"]),
    ("ortho", &["ortho"]),
    ("qsc", &["Quadrilateralized Spherical Cube", "Quadrilateralized_Spherical_Cube", "qsc"]),
    ("robin", &["Robinson", "robin"]),
    ("geocent", &["Geocentric", "geocentric", "geocent", "Geocent"]),
    ("tpers", &["Tilted_Perspective", "tpers"]),
    ("geos", &["Geostationary Satellite View", "Geostationary_Satellite", "geos"]),
    ("eqearth", &["eqearth", "Equal Earth", "Equal_Earth"]),
    ("bonne", &["bonne", "Bonne (Werner lat_1=90)"]),
    ("ob_tran", &["General Oblique Transformation", "General_Oblique_Transformation", "ob_tran"]),
];

#[derive(Clone, Debug, Default)]
pub struct Tmerc {
    en: [f64; 5],
    ml0: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Etmerc {
    cbg: [f64; 6],
    cgb: [f64; 6],
    utg: [f64; 6],
    gtu: [f64; 6],
    qn: f64,
    zb: f64,
    /// `+approx`: tmerc's forward / inverse
    approx: Option<Tmerc>,
}
#[derive(Clone, Debug, Default)]
pub struct Gauss {
    rc: f64,
    c: f64,
    phic0: f64,
    ratexp: f64,
    k: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Sterea {
    g: Gauss,
    sinc0: f64,
    cosc0: f64,
    r2: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Stere {
    coslat0: f64,
    sinlat0: f64,
    con: f64,
    cons: f64,
    ms1: f64,
    x0_: f64,
    cos_x0: f64,
    sin_x0: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Somerc {
    lambda0: f64,
    r: f64,
    alpha: f64,
    b0: f64,
    k: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Omerc {
    no_off: bool,
    no_rot: bool,
    a_: f64,
    b_: f64,
    e_: f64,
    lam0: f64,
    singam: f64,
    cosgam: f64,
    sinrot: f64,
    cosrot: f64,
    r_b: f64,
    ar_b: f64,
    br_a: f64,
    u_0: f64,
    v_pole_n: f64,
    v_pole_s: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Lcc {
    ns: f64,
    f0: f64,
    rh: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Krovak {
    s45: f64,
    alfa: f64,
    k: f64,
    n_: f64,
    ro0: f64,
    ad: f64,
    s0: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Cass {
    e0: f64,
    e1: f64,
    e2: f64,
    e3: f64,
    ml0: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Laea {
    mode: i32,
    qp: f64,
    mmf: f64,
    apa: [f64; 3],
    rq: f64,
    dd: f64,
    xmf: f64,
    ymf: f64,
    sinb1: f64,
    cosb1: f64,
    sinph0: f64,
    cosph0: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Aea {
    e3: f64,
    ns0: f64,
    c: f64,
    rh: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Gnom {
    sin_p14: f64,
    cos_p14: f64,
    infinity_dist: f64,
    rc: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Eqc {
    rc: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Poly {
    e0: f64,
    e1: f64,
    e2: f64,
    e3: f64,
    ml0: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Sinu {
    en: [f64; 5],
    n_: f64,
    m: f64,
    c_x: f64,
    c_y: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Eqdc {
    e0: f64,
    e1: f64,
    e2: f64,
    e3: f64,
    ns: f64,
    g: f64,
    rh: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Aeqd {
    sin_p12: f64,
    cos_p12: f64,
    f: f64,
}
#[derive(Clone, Debug, Default)]
pub struct Eqearth {
    apa: [f64; 3],
    qp: f64,
    rqda: f64,
}

/// A projection's own state after `init`.
#[derive(Clone, Debug, Default)]
pub enum Kind {
    #[default]
    None,
    Merc,
    Longlat,
    Tmerc(Tmerc),
    Etmerc(Etmerc),
    Sterea(Sterea),
    Stere(Stere),
    Somerc(Somerc),
    Omerc(Omerc),
    Lcc(Lcc),
    Krovak(Krovak),
    Cass(Cass),
    Laea(Laea),
    Aea(Aea),
    Gnom(Gnom),
    Cea,
    Eqc(Eqc),
    Poly(Poly),
    Nzmg,
    Mill,
    Sinu(Sinu),
    Moll,
    Eqdc(Eqdc),
    Aeqd(Aeqd),
    Eqearth(Eqearth),
}

const S_POLE: i32 = 1;
const N_POLE: i32 = 2;
const EQUIT: i32 = 3;
const OBLIQ: i32 = 4;

fn or0(v: Option<f64>) -> f64 {
    if truthy(v) { n(v) } else { 0.0 }
}
fn def0(v: Option<f64>) -> Option<f64> {
    Some(v.unwrap_or(0.0))
}

fn tmerc_init(p: &mut Proj) -> Tmerc {
    p.x0 = def0(p.x0);
    p.y0 = def0(p.y0);
    p.long0 = def0(p.long0);
    p.lat0 = def0(p.lat0);
    let mut t = Tmerc::default();
    if p.es != 0.0 && !p.es.is_nan() {
        t.en = pj_enfn(p.es);
        let l0 = n(p.lat0);
        t.ml0 = pj_mlfn(l0, sin(l0), cos(l0), &t.en);
    }
    t
}

fn etmerc_init(p: &mut Proj) -> R<Etmerc> {
    if !p.approx && (p.es.is_nan() || p.es <= 0.0) {
        return Err(
            "Incorrect elliptical usage. Try using the +approx option in the proj string, or PROJECTION[\"Fast_Transverse_Mercator\"] in the WKT."
                .into(),
        );
    }
    let mut st = Etmerc::default();
    if p.approx {
        st.approx = Some(tmerc_init(p));
    }
    p.x0 = def0(p.x0);
    p.y0 = def0(p.y0);
    p.long0 = def0(p.long0);
    p.lat0 = def0(p.lat0);
    p.k0 = Some(p.k0.unwrap_or(1.0));
    let f = p.es / (1.0 + sqrt(1.0 - p.es));
    let nn = f / (2.0 - f);
    let mut np = nn;
    st.cgb[0] = nn * (2.0 + nn * (-2.0 / 3.0 + nn * (-2.0 + nn * (116.0 / 45.0 + nn * (26.0 / 45.0 + nn * (-2854.0 / 675.0))))));
    st.cbg[0] = nn * (-2.0 + nn * (2.0 / 3.0 + nn * (4.0 / 3.0 + nn * (-82.0 / 45.0 + nn * (32.0 / 45.0 + nn * (4642.0 / 4725.0))))));
    np *= nn;
    st.cgb[1] = np * (7.0 / 3.0 + nn * (-8.0 / 5.0 + nn * (-227.0 / 45.0 + nn * (2704.0 / 315.0 + nn * (2323.0 / 945.0)))));
    st.cbg[1] = np * (5.0 / 3.0 + nn * (-16.0 / 15.0 + nn * (-13.0 / 9.0 + nn * (904.0 / 315.0 + nn * (-1522.0 / 945.0)))));
    np *= nn;
    st.cgb[2] = np * (56.0 / 15.0 + nn * (-136.0 / 35.0 + nn * (-1262.0 / 105.0 + nn * (73814.0 / 2835.0))));
    st.cbg[2] = np * (-26.0 / 15.0 + nn * (34.0 / 21.0 + nn * (8.0 / 5.0 + nn * (-12686.0 / 2835.0))));
    np *= nn;
    st.cgb[3] = np * (4279.0 / 630.0 + nn * (-332.0 / 35.0 + nn * (-399572.0 / 14175.0)));
    st.cbg[3] = np * (1237.0 / 630.0 + nn * (-12.0 / 5.0 + nn * (-24832.0 / 14175.0)));
    np *= nn;
    st.cgb[4] = np * (4174.0 / 315.0 + nn * (-144838.0 / 6237.0));
    st.cbg[4] = np * (-734.0 / 315.0 + nn * (109598.0 / 31185.0));
    np *= nn;
    st.cgb[5] = np * (601676.0 / 22275.0);
    st.cbg[5] = np * (444337.0 / 155925.0);
    np = pow(nn, 2.0);
    st.qn = n(p.k0) / (1.0 + nn) * (1.0 + np * (1.0 / 4.0 + np * (1.0 / 64.0 + np / 256.0)));
    st.utg[0] =
        nn * (-0.5 + nn * (2.0 / 3.0 + nn * (-37.0 / 96.0 + nn * (1.0 / 360.0 + nn * (81.0 / 512.0 + nn * (-96199.0 / 604800.0))))));
    st.gtu[0] = nn * (0.5 + nn * (-2.0 / 3.0 + nn * (5.0 / 16.0 + nn * (41.0 / 180.0 + nn * (-127.0 / 288.0 + nn * (7891.0 / 37800.0))))));
    st.utg[1] = np * (-1.0 / 48.0 + nn * (-1.0 / 15.0 + nn * (437.0 / 1440.0 + nn * (-46.0 / 105.0 + nn * (1118711.0 / 3870720.0)))));
    st.gtu[1] = np * (13.0 / 48.0 + nn * (-3.0 / 5.0 + nn * (557.0 / 1440.0 + nn * (281.0 / 630.0 + nn * (-1983433.0 / 1935360.0)))));
    np *= nn;
    st.utg[2] = np * (-17.0 / 480.0 + nn * (37.0 / 840.0 + nn * (209.0 / 4480.0 + nn * (-5569.0 / 90720.0))));
    st.gtu[2] = np * (61.0 / 240.0 + nn * (-103.0 / 140.0 + nn * (15061.0 / 26880.0 + nn * (167603.0 / 181440.0))));
    np *= nn;
    st.utg[3] = np * (-4397.0 / 161280.0 + nn * (11.0 / 504.0 + nn * (830251.0 / 7257600.0)));
    st.gtu[3] = np * (49561.0 / 161280.0 + nn * (-179.0 / 168.0 + nn * (6601661.0 / 7257600.0)));
    np *= nn;
    st.utg[4] = np * (-4583.0 / 161280.0 + nn * (108847.0 / 3991680.0));
    st.gtu[4] = np * (34729.0 / 80640.0 + nn * (-3418889.0 / 1995840.0));
    np *= nn;
    st.utg[5] = np * (-20648693.0 / 638668800.0);
    st.gtu[5] = np * (212378941.0 / 319334400.0);
    let z = gatg(&st.cbg, n(p.lat0));
    st.zb = -st.qn * (z + clens(&st.gtu, 2.0 * z));
    Ok(st)
}

fn gauss_init(p: &Proj) -> Gauss {
    let l0 = n(p.lat0);
    let sphi = sin(l0);
    let mut cphi = cos(l0);
    cphi *= cphi;
    let mut g =
        Gauss { rc: sqrt(1.0 - p.es) / (1.0 - p.es * sphi * sphi), c: sqrt(1.0 + p.es * cphi * cphi / (1.0 - p.es)), ..Gauss::default() };
    g.phic0 = asin(sphi / g.c);
    g.ratexp = 0.5 * g.c * p.e;
    g.k = tan(0.5 * g.phic0 + FORTPI) / (pow(tan(0.5 * l0 + FORTPI), g.c) * srat(p.e * sphi, g.ratexp));
    g
}

/// `init` of projection `pid` on `p`.
pub fn init(pid: usize, p: &mut Proj) -> R<Kind> {
    let id = REGISTRY.get(pid).map_or("", |r| r.0);
    match id {
        "merc" => {
            let con = p.b / p.a;
            p.es = 1.0 - con * con;
            if p.x0.is_none() {
                p.x0 = Some(0.0);
            }
            if p.y0.is_none() {
                p.y0 = Some(0.0);
            }
            p.long0 = Some(or0(p.long0));
            p.e = sqrt(p.es);
            if truthy(p.lat_ts) {
                let lt = n(p.lat_ts);
                p.k0 = Some(if p.sphere { cos(lt) } else { msfnz(p.e, sin(lt), cos(lt)) });
            } else if !truthy(p.k0) {
                p.k0 = if truthy(p.k) { p.k } else { Some(1.0) };
            }
            Ok(Kind::Merc)
        }
        "longlat" => Ok(Kind::Longlat),
        "tmerc" => Ok(Kind::Tmerc(tmerc_init(p))),
        "etmerc" => Ok(Kind::Etmerc(etmerc_init(p)?)),
        "utm" => {
            let zone = adjust_zone(p.zone, n(p.long0));
            let Some(zone) = zone else { return Err("unknown utm zone".into()) };
            p.lat0 = Some(0.0);
            p.long0 = Some(((6.0 * abs(zone)) - 183.0) * D2R);
            p.x0 = Some(500000.0);
            p.y0 = Some(if p.utm_south { 10000000.0 } else { 0.0 });
            p.k0 = Some(0.9996);
            Ok(Kind::Etmerc(etmerc_init(p)?))
        }
        "sterea" => {
            let g = gauss_init(p);
            let mut s = Sterea { g, ..Sterea::default() };
            if s.g.rc == 0.0 || s.g.rc.is_nan() {
                return Ok(Kind::Sterea(s));
            }
            s.sinc0 = sin(s.g.phic0);
            s.cosc0 = cos(s.g.phic0);
            s.r2 = 2.0 * s.g.rc;
            if !p.title.truthy() {
                p.title = Val::str("Oblique Stereographic Alternative");
            }
            Ok(Kind::Sterea(s))
        }
        "stere" => {
            p.x0 = Some(or0(p.x0));
            p.y0 = Some(or0(p.y0));
            p.lat0 = Some(or0(p.lat0));
            p.long0 = Some(or0(p.long0));
            let lat0 = n(p.lat0);
            let mut s = Stere { coslat0: cos(lat0), sinlat0: sin(lat0), ..Stere::default() };
            let lat_ts = n(p.lat_ts);
            if p.sphere {
                if !lat_ts.is_nan() && abs(s.coslat0) <= EPSLN {
                    p.k0 = Some(0.5 * (1.0 + sign(lat0) * sin(lat_ts)));
                }
            } else {
                if abs(s.coslat0) <= EPSLN {
                    s.con = if lat0 > 0.0 { 1.0 } else { -1.0 };
                } else {
                    s.con = f64::NAN; // undefined
                }
                s.cons = sqrt(pow(1.0 + p.e, 1.0 + p.e) * pow(1.0 - p.e, 1.0 - p.e));
                if !lat_ts.is_nan() && abs(s.coslat0) <= EPSLN && abs(cos(lat_ts)) > EPSLN {
                    p.k0 = Some(0.5 * s.cons * msfnz(p.e, sin(lat_ts), cos(lat_ts)) / tsfnz(p.e, s.con * lat_ts, s.con * sin(lat_ts)));
                }
                s.ms1 = msfnz(p.e, s.sinlat0, s.coslat0);
                s.x0_ = 2.0 * atan(ssfn(lat0, s.sinlat0, p.e)) - HALF_PI;
                s.cos_x0 = cos(s.x0_);
                s.sin_x0 = sin(s.x0_);
            }
            Ok(Kind::Stere(s))
        }
        "somerc" => {
            if !truthy(p.k0) {
                p.k0 = Some(1.0);
            }
            let phy0 = n(p.lat0);
            let lambda0 = n(p.long0);
            let sin_phy0 = sin(phy0);
            let semi = p.a;
            let inv_f = p.rf;
            let flattening = 1.0 / inv_f;
            let e2 = 2.0 * flattening - pow(flattening, 2.0);
            let e = sqrt(e2);
            p.e = e;
            let r = n(p.k0) * semi * sqrt(1.0 - e2) / (1.0 - e2 * pow(sin_phy0, 2.0));
            let alpha = sqrt(1.0 + e2 / (1.0 - e2) * pow(cos(phy0), 4.0));
            p.alpha = Some(alpha);
            let b0 = asin(sin_phy0 / alpha);
            let k1 = log(tan(PI / 4.0 + b0 / 2.0));
            let k2 = log(tan(PI / 4.0 + phy0 / 2.0));
            let k3 = log((1.0 + e * sin_phy0) / (1.0 - e * sin_phy0));
            let k = k1 - alpha * k2 + alpha * e / 2.0 * k3;
            Ok(Kind::Somerc(Somerc { lambda0, r, alpha, b0, k }))
        }
        "omerc" => omerc_init(p).map(Kind::Omerc),
        "lcc" => {
            if !truthy(p.lat2) {
                p.lat2 = p.lat1;
            }
            if !truthy(p.k0) {
                p.k0 = Some(1.0);
            }
            p.x0 = Some(or0(p.x0));
            p.y0 = Some(or0(p.y0));
            p.long0 = Some(or0(p.long0));
            let (lat1, lat2, lat0) = (n(p.lat1), n(p.lat2), n(p.lat0));
            if abs(lat1 + lat2) < EPSLN {
                return Ok(Kind::Lcc(Lcc { ns: f64::NAN, f0: f64::NAN, rh: f64::NAN }));
            }
            let temp = p.b / p.a;
            p.e = sqrt(1.0 - temp * temp);
            let sin1 = sin(lat1);
            let cos1 = cos(lat1);
            let ms1 = msfnz(p.e, sin1, cos1);
            let ts1 = tsfnz(p.e, lat1, sin1);
            let sin2 = sin(lat2);
            let cos2 = cos(lat2);
            let ms2 = msfnz(p.e, sin2, cos2);
            let ts2 = tsfnz(p.e, lat2, sin2);
            let ts0 = tsfnz(p.e, lat0, sin(lat0));
            let mut ns = if abs(lat1 - lat2) > EPSLN { log(ms1 / ms2) / log(ts1 / ts2) } else { sin1 };
            if ns.is_nan() {
                ns = sin1;
            }
            let f0 = ms1 / (ns * pow(ts1, ns));
            let rh = if abs(abs(lat0) - HALF_PI) < EPSLN { 0.0 } else { p.a * f0 * pow(ts0, ns) };
            if !p.title.truthy() {
                p.title = Val::str("Lambert Conformal Conic");
            }
            Ok(Kind::Lcc(Lcc { ns, f0, rh }))
        }
        "krovak" => {
            p.a = 6377397.155;
            p.es = 0.006674372230614;
            p.e = sqrt(p.es);
            if !truthy(p.lat0) {
                p.lat0 = Some(0.863937979737193);
            }
            if !truthy(p.long0) {
                p.long0 = Some(0.7417649320975901 - 0.308341501185665);
            }
            if !truthy(p.k0) {
                p.k0 = Some(0.9999);
            }
            let s45 = 0.785398163397448;
            let s90 = 2.0 * s45;
            let fi0 = n(p.lat0);
            let e2 = p.es;
            p.e = sqrt(e2);
            let e = p.e;
            let alfa = sqrt(1.0 + (e2 * pow(cos(fi0), 4.0)) / (1.0 - e2));
            let uq = 1.04216856380474;
            let u0 = asin(sin(fi0) / alfa);
            let g = pow((1.0 + e * sin(fi0)) / (1.0 - e * sin(fi0)), alfa * e / 2.0);
            let k = tan(u0 / 2.0 + s45) / pow(tan(fi0 / 2.0 + s45), alfa) * g;
            let k1 = n(p.k0);
            let n0 = p.a * sqrt(1.0 - e2) / (1.0 - e2 * pow(sin(fi0), 2.0));
            let s0 = 1.37008346281555;
            let nn = sin(s0);
            let ro0 = k1 * n0 / tan(s0);
            let ad = s90 - uq;
            p.k = Some(k);
            Ok(Kind::Krovak(Krovak { s45, alfa, k, n_: nn, ro0, ad, s0 }))
        }
        "cass" => {
            let mut c = Cass::default();
            if !p.sphere {
                c.e0 = e0fn(p.es);
                c.e1 = e1fn(p.es);
                c.e2 = e2fn(p.es);
                c.e3 = e3fn(p.es);
                c.ml0 = p.a * mlfn(c.e0, c.e1, c.e2, c.e3, n(p.lat0));
            } else {
                c = Cass { e0: f64::NAN, e1: f64::NAN, e2: f64::NAN, e3: f64::NAN, ml0: f64::NAN };
            }
            Ok(Kind::Cass(c))
        }
        "laea" => {
            let lat0 = n(p.lat0);
            let t = abs(lat0);
            let mut l = Laea::default();
            l.mode = if abs(t - HALF_PI) < EPSLN {
                if lat0 < 0.0 { S_POLE } else { N_POLE }
            } else if abs(t) < EPSLN {
                EQUIT
            } else {
                OBLIQ
            };
            if p.es > 0.0 {
                l.qp = qsfnz(p.e, 1.0);
                l.mmf = 0.5 / (1.0 - p.es);
                l.apa = authset(p.es);
                match l.mode {
                    N_POLE | S_POLE => l.dd = 1.0,
                    EQUIT => {
                        l.rq = sqrt(0.5 * l.qp);
                        l.dd = 1.0 / l.rq;
                        l.xmf = 1.0;
                        l.ymf = 0.5 * l.qp;
                    }
                    _ => {
                        l.rq = sqrt(0.5 * l.qp);
                        let sinphi = sin(lat0);
                        l.sinb1 = qsfnz(p.e, sinphi) / l.qp;
                        l.cosb1 = sqrt(1.0 - l.sinb1 * l.sinb1);
                        l.dd = cos(lat0) / (sqrt(1.0 - p.es * sinphi * sinphi) * l.rq * l.cosb1);
                        l.xmf = l.rq;
                        l.ymf = l.xmf / l.dd;
                        l.xmf *= l.dd;
                    }
                }
            } else if l.mode == OBLIQ {
                l.sinph0 = sin(lat0);
                l.cosph0 = cos(lat0);
            }
            Ok(Kind::Laea(l))
        }
        "aea" => {
            let (lat1, lat2, lat0) = (n(p.lat1), n(p.lat2), n(p.lat0));
            if abs(lat1 + lat2) < EPSLN {
                return Ok(Kind::Aea(Aea { e3: f64::NAN, ns0: f64::NAN, c: f64::NAN, rh: f64::NAN }));
            }
            let temp = p.b / p.a;
            p.es = 1.0 - pow(temp, 2.0);
            let e3 = sqrt(p.es);
            let mut sin_po = sin(lat1);
            let mut cos_po = cos(lat1);
            let con = sin_po;
            let ms1 = msfnz(e3, sin_po, cos_po);
            let qs1 = qsfnz(e3, sin_po);
            sin_po = sin(lat2);
            cos_po = cos(lat2);
            let ms2 = msfnz(e3, sin_po, cos_po);
            let qs2 = qsfnz(e3, sin_po);
            sin_po = sin(lat0);
            let qs0 = qsfnz(e3, sin_po);
            let ns0 = if abs(lat1 - lat2) > EPSLN { (ms1 * ms1 - ms2 * ms2) / (qs2 - qs1) } else { con };
            let c = ms1 * ms1 + ns0 * qs1;
            let rh = p.a * sqrt(c - ns0 * qs0) / ns0;
            Ok(Kind::Aea(Aea { e3, ns0, c, rh }))
        }
        "gnom" => {
            let lat0 = n(p.lat0);
            Ok(Kind::Gnom(Gnom { sin_p14: sin(lat0), cos_p14: cos(lat0), infinity_dist: 1000.0 * p.a, rc: 1.0 }))
        }
        "cea" => {
            if !p.sphere {
                let lt = n(p.lat_ts);
                p.k0 = Some(msfnz(p.e, sin(lt), cos(lt)));
            }
            Ok(Kind::Cea)
        }
        "eqc" => {
            p.x0 = Some(or0(p.x0));
            p.y0 = Some(or0(p.y0));
            p.lat0 = Some(or0(p.lat0));
            p.long0 = Some(or0(p.long0));
            p.lat_ts = Some(or0(p.lat_ts));
            if !p.title.truthy() {
                p.title = Val::str("Equidistant Cylindrical (Plate Carre)");
            }
            Ok(Kind::Eqc(Eqc { rc: cos(n(p.lat_ts)) }))
        }
        "poly" => {
            let temp = p.b / p.a;
            p.es = 1.0 - pow(temp, 2.0);
            p.e = sqrt(p.es);
            let mut q = Poly { e0: e0fn(p.es), e1: e1fn(p.es), e2: e2fn(p.es), e3: e3fn(p.es), ml0: 0.0 };
            q.ml0 = p.a * mlfn(q.e0, q.e1, q.e2, q.e3, n(p.lat0));
            Ok(Kind::Poly(q))
        }
        "nzmg" => Ok(Kind::Nzmg),
        "mill" => Ok(Kind::Mill),
        "sinu" => {
            p.long0 = Some(or0(p.long0));
            let mut s = Sinu::default();
            if !p.sphere {
                s.en = pj_enfn(p.es);
                s.n_ = f64::NAN;
                s.m = f64::NAN;
                s.c_x = f64::NAN;
                s.c_y = f64::NAN;
            } else {
                s.n_ = 1.0;
                s.m = 0.0;
                p.es = 0.0;
                s.c_y = sqrt((s.m + 1.0) / s.n_);
                s.c_x = s.c_y / (s.m + 1.0);
            }
            Ok(Kind::Sinu(s))
        }
        "eck6" => {
            p.sphere = true;
            p.b = p.a;
            let m = 1.0;
            let nn = 2.570796326794896619231321691;
            p.es = 0.0;
            let c_y = sqrt((m + 1.0) / nn);
            Ok(Kind::Sinu(Sinu { en: [0.0; 5], n_: nn, m, c_x: c_y / (m + 1.0), c_y }))
        }
        "moll" => {
            p.x0 = def0(p.x0);
            p.y0 = def0(p.y0);
            p.long0 = def0(p.long0);
            Ok(Kind::Moll)
        }
        "eqdc" => {
            let (lat1, lat2) = (n(p.lat1), n(p.lat2));
            if abs(lat1 + lat2) < EPSLN {
                return Ok(Kind::Eqdc(Eqdc {
                    e0: f64::NAN,
                    e1: f64::NAN,
                    e2: f64::NAN,
                    e3: f64::NAN,
                    ns: f64::NAN,
                    g: f64::NAN,
                    rh: f64::NAN,
                }));
            }
            p.lat2 = if truthy(p.lat2) { p.lat2 } else { p.lat1 };
            let lat2 = n(p.lat2);
            let temp = p.b / p.a;
            p.es = 1.0 - pow(temp, 2.0);
            p.e = sqrt(p.es);
            let mut q = Eqdc { e0: e0fn(p.es), e1: e1fn(p.es), e2: e2fn(p.es), e3: e3fn(p.es), ..Eqdc::default() };
            let mut sin_phi = sin(lat1);
            let mut cos_phi = cos(lat1);
            let ms1 = msfnz(p.e, sin_phi, cos_phi);
            let ml1 = mlfn(q.e0, q.e1, q.e2, q.e3, lat1);
            if abs(lat1 - lat2) < EPSLN {
                q.ns = sin_phi;
            } else {
                sin_phi = sin(lat2);
                cos_phi = cos(lat2);
                let ms2 = msfnz(p.e, sin_phi, cos_phi);
                let ml2 = mlfn(q.e0, q.e1, q.e2, q.e3, lat2);
                q.ns = (ms1 - ms2) / (ml2 - ml1);
            }
            q.g = ml1 + ms1 / q.ns;
            let ml0 = mlfn(q.e0, q.e1, q.e2, q.e3, n(p.lat0));
            q.rh = p.a * (q.g - ml0);
            Ok(Kind::Eqdc(q))
        }
        "aeqd" => {
            let lat0 = n(p.lat0);
            let a = Aeqd { sin_p12: sin(lat0), cos_p12: cos(lat0), f: p.es / (1.0 + sqrt(1.0 - p.es)) };
            p.x0 = Some(or0(p.x0));
            p.y0 = Some(or0(p.y0));
            p.long0 = Some(or0(p.long0));
            Ok(Kind::Aeqd(a))
        }
        "eqearth" => {
            p.long0 = def0(p.long0);
            p.x0 = def0(p.x0);
            p.y0 = def0(p.y0);
            let mut q = Eqearth::default();
            if p.es != 0.0 {
                q.apa = authset(p.es);
                q.qp = qsfnz(p.e, 1.0);
                q.rqda = sqrt(0.5 * q.qp);
            }
            Ok(Kind::Eqearth(q))
        }
        other => Err(format!("Projection {other} is not ported to the Rust fork yet")),
    }
}

fn ssfn(phit: f64, sinphi: f64, eccen: f64) -> f64 {
    let sinphi = sinphi * eccen;
    tan(0.5 * (HALF_PI + phit)) * pow((1.0 - sinphi) / (1.0 + sinphi), 0.5 * eccen)
}

fn omerc_init(p: &mut Proj) -> R<Omerc> {
    const TOL: f64 = 1e-7;
    let mut st = Omerc::default();
    let mut gamma = 0.0;
    let (mut lamc, mut lam1, mut lam2, mut phi1, mut phi2, mut alpha_c) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    if !truthy(p.k0) {
        p.k0 = Some(1.0);
    }
    // isTypeA
    let type_a = ["Hotine_Oblique_Mercator", "Hotine_Oblique_Mercator_variant_A", "Hotine_Oblique_Mercator_Azimuth_Natural_Origin"];
    let pname = match &p.proj_name {
        Val::Obj(o) => o.keys().first().cloned().map_or(Val::Undef, Val::Str),
        other => other.clone(),
    };
    let pname_s = pname.to_js_string();
    let norm = match &pname {
        Val::Str(s) => crate::proj::normalized_proj_name(s),
        _ => return Err(crate::defs::type_error_call(&pname, "replace")),
    };
    st.no_off = p.has("no_uoff") || p.has("no_off") || type_a.contains(&pname_s.as_str()) || type_a.contains(&norm.as_str());
    st.no_rot = p.has("no_rot");
    let mut alp = p.alpha.is_some();
    let gam = p.rectified_grid_angle.is_some();
    if alp {
        alpha_c = n(p.alpha);
    }
    if gam {
        gamma = n(p.rectified_grid_angle);
        if !alp {
            alpha_c = 0.0;
            alp = true;
        }
    }
    let lat0 = n(p.lat0);
    if alp || gam {
        lamc = n(p.longc);
    } else {
        lam1 = n(p.long1);
        phi1 = n(p.lat1);
        lam2 = n(p.long2);
        phi2 = n(p.lat2);
        let con = abs(phi1);
        if abs(phi1 - phi2) <= TOL
            || con <= TOL
            || abs(con - HALF_PI) <= TOL
            || abs(abs(lat0) - HALF_PI) <= TOL
            || abs(abs(phi2) - HALF_PI) <= TOL
        {
            return Err(String::new());
        }
    }
    let one_es = 1.0 - p.es;
    let com = sqrt(one_es);
    let (d, mut f);
    if abs(lat0) > EPSLN {
        let sinph0 = sin(lat0);
        let cosph0 = cos(lat0);
        let con = 1.0 - p.es * sinph0 * sinph0;
        st.b_ = cosph0 * cosph0;
        st.b_ = sqrt(1.0 + p.es * st.b_ * st.b_ / one_es);
        st.a_ = st.b_ * n(p.k0) * com / con;
        d = st.b_ * com / (cosph0 * sqrt(con));
        f = d * d - 1.0;
        if f <= 0.0 {
            f = 0.0;
        } else {
            f = sqrt(f);
            if lat0 < 0.0 {
                f = -f;
            }
        }
        f += d;
        st.e_ = f;
        st.e_ *= pow(tsfnz(p.e, lat0, sinph0), st.b_);
    } else {
        st.b_ = 1.0 / com;
        st.a_ = n(p.k0);
        d = 1.0;
        f = 1.0;
        st.e_ = 1.0;
    }
    let gamma0;
    if alp || gam {
        if alp {
            gamma0 = asin(sin(alpha_c) / d);
            if !gam {
                gamma = alpha_c;
            }
        } else {
            gamma0 = gamma;
            alpha_c = asin(d * sin(gamma0));
        }
        st.lam0 = lamc - asin(0.5 * (f - 1.0 / f) * tan(gamma0)) / st.b_;
    } else {
        let h = pow(tsfnz(p.e, phi1, sin(phi1)), st.b_);
        let l = pow(tsfnz(p.e, phi2, sin(phi2)), st.b_);
        f = st.e_ / h;
        let pp = (l - h) / (l + h);
        let mut j = st.e_ * st.e_;
        j = (j - l * h) / (j + l * h);
        let con = lam1 - lam2;
        if con < -PI {
            lam2 -= TWO_PI;
        } else if con > PI {
            lam2 += TWO_PI;
        }
        st.lam0 = adjust_lon(0.5 * (lam1 + lam2) - atan(j * tan(0.5 * st.b_ * (lam1 - lam2)) / pp) / st.b_, p.over);
        gamma0 = atan(2.0 * sin(st.b_ * adjust_lon(lam1 - st.lam0, p.over)) / (f - 1.0 / f));
        alpha_c = asin(d * sin(gamma0));
        gamma = alpha_c;
    }
    st.singam = sin(gamma0);
    st.cosgam = cos(gamma0);
    st.sinrot = sin(gamma);
    st.cosrot = cos(gamma);
    st.r_b = 1.0 / st.b_;
    st.ar_b = st.a_ * st.r_b;
    st.br_a = 1.0 / st.ar_b;
    if st.no_off {
        st.u_0 = 0.0;
    } else {
        st.u_0 = abs(st.ar_b * atan(sqrt(d * d - 1.0) / cos(alpha_c)));
        if lat0 < 0.0 {
            st.u_0 = -st.u_0;
        }
    }
    let ff = 0.5 * gamma0;
    st.v_pole_n = st.ar_b * log(tan(FORTPI - ff));
    st.v_pole_s = st.ar_b * log(tan(FORTPI + ff));
    Ok(st)
}

fn tmerc_forward(p: &Proj, t: &Tmerc, mut pt: Pt) -> Out {
    let lon = pt.x;
    let lat = pt.y;
    let delta_lon = adjust_lon(lon - n(p.long0), p.over);
    let sin_phi = sin(lat);
    let cos_phi = cos(lat);
    let (x, y);
    if p.es == 0.0 || p.es.is_nan() {
        let mut b = cos_phi * sin(delta_lon);
        if abs(abs(b) - 1.0) < EPSLN {
            return Out::Num(93.0);
        }
        x = 0.5 * p.a * n(p.k0) * log((1.0 + b) / (1.0 - b)) + n(p.x0);
        let mut yy = cos_phi * cos(delta_lon) / sqrt(1.0 - pow(b, 2.0));
        b = abs(yy);
        if b >= 1.0 {
            if (b - 1.0) > EPSLN {
                return Out::Num(93.0);
            }
            yy = 0.0;
        } else {
            yy = acos(yy);
        }
        if lat < 0.0 {
            yy = -yy;
        }
        y = p.a * n(p.k0) * (yy - n(p.lat0)) + n(p.y0);
    } else {
        let mut al = cos_phi * delta_lon;
        let als = pow(al, 2.0);
        let c = p.ep2 * pow(cos_phi, 2.0);
        let cs = pow(c, 2.0);
        let tq = if abs(cos_phi) > EPSLN { tan(lat) } else { 0.0 };
        let tt = pow(tq, 2.0);
        let ts = pow(tt, 2.0);
        let con = 1.0 - p.es * pow(sin_phi, 2.0);
        al /= sqrt(con);
        let ml = pj_mlfn(lat, sin_phi, cos_phi, &t.en);
        x = p.a
            * (n(p.k0)
                * al
                * (1.0
                    + als / 6.0
                        * (1.0 - tt
                            + c
                            + als / 20.0
                                * (5.0 - 18.0 * tt + ts + 14.0 * c - 58.0 * tt * c
                                    + als / 42.0 * (61.0 + 179.0 * ts - ts * tt - 479.0 * tt)))))
            + n(p.x0);
        y = p.a
            * (n(p.k0)
                * (ml - t.ml0
                    + sin_phi * delta_lon * al / 2.0
                        * (1.0
                            + als / 12.0
                                * (5.0 - tt
                                    + 9.0 * c
                                    + 4.0 * cs
                                    + als / 30.0
                                        * (61.0 + ts - 58.0 * tt + 270.0 * c - 330.0 * tt * c
                                            + als / 56.0 * (1385.0 + 543.0 * ts - ts * tt - 3111.0 * tt))))))
            + n(p.y0);
    }
    pt.x = x;
    pt.y = y;
    Out::P(pt)
}

fn tmerc_inverse(p: &Proj, t: &Tmerc, mut pt: Pt) -> Out {
    let x = (pt.x - n(p.x0)) * (1.0 / p.a);
    let y = (pt.y - n(p.y0)) * (1.0 / p.a);
    let (lat, lon);
    if p.es == 0.0 || p.es.is_nan() {
        let f = exp(x / n(p.k0));
        let g = 0.5 * (f - 1.0 / f);
        let temp = n(p.lat0) + y / n(p.k0);
        let h = cos(temp);
        let con = sqrt((1.0 - pow(h, 2.0)) / (1.0 + pow(g, 2.0)));
        let mut la = asin(con);
        if y < 0.0 {
            la = -la;
        }
        lat = la;
        lon = if g == 0.0 && h == 0.0 { 0.0 } else { adjust_lon(atan2(g, h) + n(p.long0), p.over) };
    } else {
        let con0 = t.ml0 + y / n(p.k0);
        let phi = pj_inv_mlfn(con0, p.es, &t.en);
        if abs(phi) < HALF_PI {
            let sin_phi = sin(phi);
            let cos_phi = cos(phi);
            let tan_phi = if abs(cos_phi) > EPSLN { tan(phi) } else { 0.0 };
            let c = p.ep2 * pow(cos_phi, 2.0);
            let cs = pow(c, 2.0);
            let tt = pow(tan_phi, 2.0);
            let ts = pow(tt, 2.0);
            let mut con = 1.0 - p.es * pow(sin_phi, 2.0);
            let d = x * sqrt(con) / n(p.k0);
            let ds = pow(d, 2.0);
            con *= tan_phi;
            lat = phi
                - (con * ds / (1.0 - p.es))
                    * 0.5
                    * (1.0
                        - ds / 12.0
                            * (5.0 + 3.0 * tt - 9.0 * c * tt + c
                                - 4.0 * cs
                                - ds / 30.0
                                    * (61.0 + 90.0 * tt - 252.0 * c * tt + 45.0 * ts + 46.0 * c
                                        - ds / 56.0 * (1385.0 + 3633.0 * tt + 4095.0 * ts + 1574.0 * ts * tt))));
            lon = adjust_lon(
                n(p.long0)
                    + (d * (1.0
                        - ds / 6.0
                            * (1.0 + 2.0 * tt + c
                                - ds / 20.0
                                    * (5.0 + 28.0 * tt + 24.0 * ts + 8.0 * c * tt + 6.0 * c
                                        - ds / 42.0 * (61.0 + 662.0 * tt + 1320.0 * ts + 720.0 * ts * tt))))
                        / cos_phi),
                p.over,
            );
        } else {
            lat = HALF_PI * sign(y);
            lon = 0.0;
        }
    }
    pt.x = lon;
    pt.y = lat;
    Out::P(pt)
}

fn gauss_forward(g: &Gauss, e: f64, pt: &mut Pt) {
    let lon = pt.x;
    let lat = pt.y;
    pt.y = 2.0 * atan(g.k * pow(tan(0.5 * lat + FORTPI), g.c) * srat(e * sin(lat), g.ratexp)) - HALF_PI;
    pt.x = g.c * lon;
}
fn gauss_inverse(g: &Gauss, e: f64, pt: &mut Pt) -> bool {
    const DEL_TOL: f64 = 1e-14;
    let lon = pt.x / g.c;
    let mut lat = pt.y;
    let num = pow(tan(0.5 * lat + FORTPI) / g.k, 1.0 / g.c);
    let mut i = 20;
    while i > 0 {
        lat = 2.0 * atan(num * srat(e * sin(pt.y), -0.5 * e)) - HALF_PI;
        if abs(lat - pt.y) < DEL_TOL {
            break;
        }
        pt.y = lat;
        i -= 1;
    }
    if i == 0 {
        return false;
    }
    pt.x = lon;
    pt.y = lat;
    true
}

/// `forward(p)` of the projection.
pub fn forward(p: &Proj, mut pt: Pt) -> R<Out> {
    let long0 = n(p.long0);
    let (x0, y0) = (n(p.x0), n(p.y0));
    let k0 = n(p.k0);
    Ok(match &p.kind {
        Kind::None => return Err("p.forward is not a function".into()),
        Kind::Longlat => Out::P(pt),
        Kind::Merc => {
            let lon = pt.x;
            let lat = pt.y;
            // proj4's range check here (`lat > 90 && lat < -90 && …`) can never be true: left out
            if abs(abs(lat) - HALF_PI) <= EPSLN {
                return Ok(Out::Null);
            }
            let (x, y);
            if p.sphere {
                x = x0 + p.a * k0 * adjust_lon(lon - long0, p.over);
                y = y0 + p.a * k0 * log(tan(FORTPI + 0.5 * lat));
            } else {
                let sinphi = sin(lat);
                let ts = tsfnz(p.e, lat, sinphi);
                x = x0 + p.a * k0 * adjust_lon(lon - long0, p.over);
                y = y0 - p.a * k0 * log(ts);
            }
            pt.x = x;
            pt.y = y;
            Out::P(pt)
        }
        Kind::Tmerc(t) => tmerc_forward(p, t, pt),
        Kind::Etmerc(st) => {
            if let Some(t) = &st.approx {
                return Ok(tmerc_forward(p, t, pt));
            }
            let mut ce = adjust_lon(pt.x - long0, p.over);
            let mut cn = pt.y;
            cn = gatg(&st.cbg, cn);
            let sin_cn = sin(cn);
            let cos_cn = cos(cn);
            let sin_ce = sin(ce);
            let cos_ce = cos(ce);
            cn = atan2(sin_cn, cos_ce * cos_cn);
            ce = atan2(sin_ce * cos_cn, hypot(sin_cn, cos_cn * cos_ce));
            ce = asinhy(tan(ce));
            let tmp = clens_cmplx(&st.gtu, 2.0 * cn, 2.0 * ce);
            cn += tmp[0];
            ce += tmp[1];
            let (x, y);
            if abs(ce) <= 2.623395162778 {
                x = p.a * (st.qn * ce) + x0;
                y = p.a * (st.qn * cn + st.zb) + y0;
            } else {
                x = f64::INFINITY;
                y = f64::INFINITY;
            }
            pt.x = x;
            pt.y = y;
            Out::P(pt)
        }
        Kind::Sterea(s) => {
            pt.x = adjust_lon(pt.x - long0, p.over);
            gauss_forward(&s.g, p.e, &mut pt);
            let sinc = sin(pt.y);
            let cosc = cos(pt.y);
            let cosl = cos(pt.x);
            let k = k0 * s.r2 / (1.0 + s.sinc0 * sinc + s.cosc0 * cosc * cosl);
            pt.x = k * cosc * sin(pt.x);
            pt.y = k * (s.cosc0 * sinc - s.sinc0 * cosc * cosl);
            pt.x = p.a * pt.x + x0;
            pt.y = p.a * pt.y + y0;
            Out::P(pt)
        }
        Kind::Stere(s) => {
            let lon = pt.x;
            let lat = pt.y;
            let lat0 = n(p.lat0);
            let sinlat = sin(lat);
            let coslat = cos(lat);
            let dlon = adjust_lon(lon - long0, p.over);
            if abs(abs(lon - long0) - PI) <= EPSLN && abs(lat + lat0) <= EPSLN {
                pt.x = f64::NAN;
                pt.y = f64::NAN;
                return Ok(Out::P(pt));
            }
            if p.sphere {
                let a = 2.0 * k0 / (1.0 + s.sinlat0 * sinlat + s.coslat0 * coslat * cos(dlon));
                pt.x = p.a * a * coslat * sin(dlon) + x0;
                pt.y = p.a * a * (s.coslat0 * sinlat - s.sinlat0 * coslat * cos(dlon)) + y0;
                return Ok(Out::P(pt));
            }
            let x = 2.0 * atan(ssfn(lat, sinlat, p.e)) - HALF_PI;
            let cos_x = cos(x);
            let sin_x = sin(x);
            let a;
            if abs(s.coslat0) <= EPSLN {
                let ts = tsfnz(p.e, lat * s.con, s.con * sinlat);
                let rh = 2.0 * p.a * k0 * ts / s.cons;
                pt.x = x0 + rh * sin(lon - long0);
                pt.y = y0 - s.con * rh * cos(lon - long0);
                return Ok(Out::P(pt));
            } else if abs(s.sinlat0) < EPSLN {
                a = 2.0 * p.a * k0 / (1.0 + cos_x * cos(dlon));
                pt.y = a * sin_x;
            } else {
                a = 2.0 * p.a * k0 * s.ms1 / (s.cos_x0 * (1.0 + s.sin_x0 * sin_x + s.cos_x0 * cos_x * cos(dlon)));
                pt.y = a * (s.cos_x0 * sin_x - s.sin_x0 * cos_x * cos(dlon)) + y0;
            }
            pt.x = a * cos_x * sin(dlon) + x0;
            Out::P(pt)
        }
        Kind::Somerc(s) => {
            let sa1 = log(tan(PI / 4.0 - pt.y / 2.0));
            let sa2 = p.e / 2.0 * log((1.0 + p.e * sin(pt.y)) / (1.0 - p.e * sin(pt.y)));
            let ss = -s.alpha * (sa1 + sa2) + s.k;
            let b = 2.0 * (atan(exp(ss)) - PI / 4.0);
            let i = s.alpha * (pt.x - s.lambda0);
            let rot_i = atan(sin(i) / (sin(s.b0) * tan(b) + cos(s.b0) * cos(i)));
            let rot_b = asin(cos(s.b0) * sin(b) - sin(s.b0) * cos(b) * cos(i));
            pt.y = s.r / 2.0 * log((1.0 + sin(rot_b)) / (1.0 - sin(rot_b))) + y0;
            pt.x = s.r * rot_i + x0;
            Out::P(pt)
        }
        Kind::Omerc(st) => {
            let mut c = Pt { x: f64::NAN, y: f64::NAN, z: None, m: None };
            pt.x -= st.lam0;
            let (u, v);
            if abs(abs(pt.y) - HALF_PI) > EPSLN {
                let w = st.e_ / pow(tsfnz(p.e, pt.y, sin(pt.y)), st.b_);
                let temp = 1.0 / w;
                let ss = 0.5 * (w - temp);
                let tt = 0.5 * (w + temp);
                let vv = sin(st.b_ * pt.x);
                let uu = (ss * st.singam - vv * st.cosgam) / tt;
                if abs(abs(uu) - 1.0) < EPSLN {
                    return Err(String::new());
                }
                v = 0.5 * st.ar_b * log((1.0 - uu) / (1.0 + uu));
                let temp2 = cos(st.b_ * pt.x);
                u = if abs(temp2) < 1e-7 { st.a_ * pt.x } else { st.ar_b * atan2(ss * st.cosgam + vv * st.singam, temp2) };
            } else {
                v = if pt.y > 0.0 { st.v_pole_n } else { st.v_pole_s };
                u = st.ar_b * pt.y;
            }
            if st.no_rot {
                c.x = u;
                c.y = v;
            } else {
                let u2 = u - st.u_0;
                c.x = v * st.cosrot + u2 * st.sinrot;
                c.y = u2 * st.cosrot - v * st.sinrot;
            }
            c.x = p.a * c.x + x0;
            c.y = p.a * c.y + y0;
            c.z = pt.z;
            c.m = pt.m;
            Out::P(c)
        }
        Kind::Lcc(l) => {
            let lon = pt.x;
            let mut lat = pt.y;
            if abs(2.0 * abs(lat) - PI) <= EPSLN {
                lat = sign(lat) * (HALF_PI - 2.0 * EPSLN);
            }
            let mut con = abs(abs(lat) - HALF_PI);
            let rh1;
            if con > EPSLN {
                let ts = tsfnz(p.e, lat, sin(lat));
                rh1 = p.a * l.f0 * pow(ts, l.ns);
            } else {
                con = lat * l.ns;
                if con <= 0.0 {
                    return Ok(Out::Null);
                }
                rh1 = 0.0;
            }
            let theta = l.ns * adjust_lon(lon - long0, p.over);
            pt.x = k0 * (rh1 * sin(theta)) + x0;
            pt.y = k0 * (l.rh - rh1 * cos(theta)) + y0;
            Out::P(pt)
        }
        Kind::Krovak(k) => {
            let lon = pt.x;
            let lat = pt.y;
            let delta_lon = adjust_lon(lon - long0, p.over);
            let gfi = pow((1.0 + p.e * sin(lat)) / (1.0 - p.e * sin(lat)), k.alfa * p.e / 2.0);
            let u = 2.0 * (atan(k.k * pow(tan(lat / 2.0 + k.s45), k.alfa) / gfi) - k.s45);
            let deltav = -delta_lon * k.alfa;
            let s = asin(cos(k.ad) * sin(u) + sin(k.ad) * cos(u) * cos(deltav));
            let d = asin(cos(u) * sin(deltav) / cos(s));
            let eps = k.n_ * d;
            let ro = k.ro0 * pow(tan(k.s0 / 2.0 + k.s45), k.n_) / pow(tan(s / 2.0 + k.s45), k.n_);
            pt.y = ro * cos(eps) / 1.0;
            pt.x = ro * sin(eps) / 1.0;
            if !p.czech {
                pt.y *= -1.0;
                pt.x *= -1.0;
            }
            Out::P(pt)
        }
        Kind::Cass(c) => {
            let mut lam = pt.x;
            let phi = pt.y;
            lam = adjust_lon(lam - long0, p.over);
            let (x, y);
            if p.sphere {
                x = p.a * asin(cos(phi) * sin(lam));
                y = p.a * (atan2(tan(phi), cos(lam)) - n(p.lat0));
            } else {
                let sinphi = sin(phi);
                let cosphi = cos(phi);
                let nl = g_n(p.a, p.e, sinphi);
                let tl = tan(phi) * tan(phi);
                let al = lam * cos(phi);
                let asq = al * al;
                let cl = p.es * cosphi * cosphi / (1.0 - p.es);
                let ml = p.a * mlfn(c.e0, c.e1, c.e2, c.e3, phi);
                x = nl * al * (1.0 - asq * tl * (1.0 / 6.0 - (8.0 - tl + 8.0 * cl) * asq / 120.0));
                y = ml - c.ml0 + nl * sinphi / cosphi * asq * (0.5 + (5.0 - tl + 6.0 * cl) * asq / 24.0);
            }
            pt.x = x + x0;
            pt.y = y + y0;
            Out::P(pt)
        }
        Kind::Laea(l) => return Ok(laea_forward(p, l, pt)),
        Kind::Aea(a) => {
            let lon = pt.x;
            let lat = pt.y;
            let sin_phi = sin(lat);
            let qs = qsfnz(a.e3, sin_phi);
            let rh1 = p.a * sqrt(a.c - a.ns0 * qs) / a.ns0;
            let theta = a.ns0 * adjust_lon(lon - long0, p.over);
            pt.x = rh1 * sin(theta) + x0;
            pt.y = a.rh - rh1 * cos(theta) + y0;
            Out::P(pt)
        }
        Kind::Gnom(g) => {
            let lon = pt.x;
            let lat = pt.y;
            let dlon = adjust_lon(lon - long0, p.over);
            let sinphi = sin(lat);
            let cosphi = cos(lat);
            let coslon = cos(dlon);
            let gg = g.sin_p14 * sinphi + g.cos_p14 * cosphi * coslon;
            let ksp = 1.0;
            if gg > 0.0 || abs(gg) <= EPSLN {
                pt.x = x0 + p.a * ksp * cosphi * sin(dlon) / gg;
                pt.y = y0 + p.a * ksp * (g.cos_p14 * sinphi - g.sin_p14 * cosphi * coslon) / gg;
            } else {
                pt.x = x0 + g.infinity_dist * cosphi * sin(dlon);
                pt.y = y0 + g.infinity_dist * (g.cos_p14 * sinphi - g.sin_p14 * cosphi * coslon);
            }
            Out::P(pt)
        }
        Kind::Cea => {
            let lon = pt.x;
            let lat = pt.y;
            let dlon = adjust_lon(lon - long0, p.over);
            let lat_ts = n(p.lat_ts);
            if p.sphere {
                pt.x = x0 + p.a * dlon * cos(lat_ts);
                pt.y = y0 + p.a * sin(lat) / cos(lat_ts);
            } else {
                let qs = qsfnz(p.e, sin(lat));
                pt.x = x0 + p.a * k0 * dlon;
                pt.y = y0 + p.a * qs * 0.5 / k0;
            }
            Out::P(pt)
        }
        Kind::Eqc(e) => {
            let dlon = adjust_lon(pt.x - long0, p.over);
            let dlat = adjust_lat(pt.y - n(p.lat0));
            pt.x = x0 + (p.a * dlon * e.rc);
            pt.y = y0 + (p.a * dlat);
            Out::P(pt)
        }
        Kind::Poly(q) => {
            let lon = pt.x;
            let lat = pt.y;
            let dlon = adjust_lon(lon - long0, p.over);
            let el = dlon * sin(lat);
            let (x, y);
            if p.sphere {
                if abs(lat) <= EPSLN {
                    x = p.a * dlon;
                    y = -1.0 * p.a * n(p.lat0);
                } else {
                    x = p.a * sin(el) / tan(lat);
                    y = p.a * (adjust_lat(lat - n(p.lat0)) + (1.0 - cos(el)) / tan(lat));
                }
            } else if abs(lat) <= EPSLN {
                x = p.a * dlon;
                y = -1.0 * q.ml0;
            } else {
                let nl = g_n(p.a, p.e, sin(lat)) / tan(lat);
                x = nl * sin(el);
                y = p.a * mlfn(q.e0, q.e1, q.e2, q.e3, lat) - q.ml0 + nl * (1.0 - cos(el));
            }
            pt.x = x + x0;
            pt.y = y + y0;
            Out::P(pt)
        }
        Kind::Nzmg => return Ok(nzmg_forward(p, pt)),
        Kind::Mill => {
            let dlon = adjust_lon(pt.x - long0, p.over);
            let x = x0 + p.a * dlon;
            let y = y0 + p.a * log(tan((PI / 4.0) + (pt.y / 2.5))) * 1.25;
            pt.x = x;
            pt.y = y;
            Out::P(pt)
        }
        Kind::Sinu(s) => {
            let lon = adjust_lon(pt.x - long0, p.over);
            let mut lat = pt.y;
            let (x, y);
            if p.sphere {
                if s.m == 0.0 || s.m.is_nan() {
                    lat = if s.n_ != 1.0 { asin(s.n_ * sin(lat)) } else { lat };
                } else {
                    let k = s.n_ * sin(lat);
                    for _ in 0..20 {
                        let v = (s.m * lat + sin(lat) - k) / (s.m + cos(lat));
                        lat -= v;
                        if abs(v) < EPSLN {
                            break;
                        }
                    }
                }
                x = p.a * s.c_x * lon * (s.m + cos(lat));
                y = p.a * s.c_y * lat;
            } else {
                let ss = sin(lat);
                let c = cos(lat);
                y = p.a * pj_mlfn(lat, ss, c, &s.en);
                x = p.a * lon * c / sqrt(1.0 - p.es * ss * ss);
            }
            pt.x = x;
            pt.y = y;
            Out::P(pt)
        }
        Kind::Moll => {
            let lon = pt.x;
            let lat = pt.y;
            let mut delta_lon = adjust_lon(lon - long0, p.over);
            let mut theta = lat;
            let con = PI * sin(lat);
            // Spec gap: the JS iterates `while (true)` (a NaN never ends it); bounded here
            for _ in 0..100_000 {
                let dt = -(theta + sin(theta) - con) / (1.0 + cos(theta));
                theta += dt;
                if abs(dt) < EPSLN {
                    break;
                }
            }
            theta /= 2.0;
            if PI / 2.0 - abs(lat) < EPSLN {
                delta_lon = 0.0;
            }
            pt.x = 0.900316316158 * p.a * delta_lon * cos(theta) + x0;
            pt.y = 1.4142135623731 * p.a * sin(theta) + y0;
            Out::P(pt)
        }
        Kind::Eqdc(q) => {
            let lon = pt.x;
            let lat = pt.y;
            let rh1 = if p.sphere { p.a * (q.g - lat) } else { p.a * (q.g - mlfn(q.e0, q.e1, q.e2, q.e3, lat)) };
            let theta = q.ns * adjust_lon(lon - long0, p.over);
            pt.x = x0 + rh1 * sin(theta);
            pt.y = y0 + q.rh - rh1 * cos(theta);
            Out::P(pt)
        }
        Kind::Aeqd(a) => return Ok(aeqd_forward(p, a, pt)),
        Kind::Eqearth(q) => {
            const A1: f64 = 1.340264;
            const A2: f64 = -0.081106;
            const A3: f64 = 0.000893;
            const A4: f64 = 0.003796;
            let m = sqrt(3.0) / 2.0;
            let lam = adjust_lon(pt.x - long0, p.over);
            let phi = pt.y;
            let mut sinphi = sin(phi);
            if p.es != 0.0 {
                sinphi = qsfnz(p.e, sinphi) / q.qp;
            }
            let pl = asin(m * sinphi);
            let pl2 = pl * pl;
            let pl6 = pl2 * pl2 * pl2;
            pt.x = lam * cos(pl) / (m * (A1 + 3.0 * A2 * pl2 + pl6 * (7.0 * A3 + 9.0 * A4 * pl2)));
            pt.y = pl * (A1 + A2 * pl2 + pl6 * (A3 + A4 * pl2));
            if p.es != 0.0 {
                pt.x *= q.rqda;
                pt.y *= q.rqda;
            }
            pt.x = p.a * pt.x + x0;
            pt.y = p.a * pt.y + y0;
            Out::P(pt)
        }
    })
}

fn laea_forward(p: &Proj, l: &Laea, mut pt: Pt) -> Out {
    let long0 = n(p.long0);
    let lat0 = n(p.lat0);
    let mut lam = pt.x;
    let phi = pt.y;
    lam = adjust_lon(lam - long0, p.over);
    let (mut x, mut y) = (f64::NAN, f64::NAN);
    if p.sphere {
        let sinphi = sin(phi);
        let cosphi = cos(phi);
        let mut coslam = cos(lam);
        if l.mode == OBLIQ || l.mode == EQUIT {
            y = if l.mode == EQUIT { 1.0 + cosphi * coslam } else { 1.0 + l.sinph0 * sinphi + l.cosph0 * cosphi * coslam };
            if y <= EPSLN {
                return Out::Null;
            }
            y = sqrt(2.0 / y);
            x = y * cosphi * sin(lam);
            y *= if l.mode == EQUIT { sinphi } else { l.cosph0 * sinphi - l.sinph0 * cosphi * coslam };
        } else if l.mode == N_POLE || l.mode == S_POLE {
            if l.mode == N_POLE {
                coslam = -coslam;
            }
            if abs(phi + lat0) < EPSLN {
                return Out::Null;
            }
            y = FORTPI - phi * 0.5;
            y = 2.0 * (if l.mode == S_POLE { cos(y) } else { sin(y) });
            x = y * sin(lam);
            y *= coslam;
        }
    } else {
        let mut sinb = 0.0;
        let mut cosb = 0.0;
        let mut b = 0.0;
        let coslam = cos(lam);
        let sinlam = sin(lam);
        let sinphi = sin(phi);
        let mut q = qsfnz(p.e, sinphi);
        if l.mode == OBLIQ || l.mode == EQUIT {
            sinb = q / l.qp;
            cosb = sqrt(1.0 - sinb * sinb);
        }
        match l.mode {
            OBLIQ => b = 1.0 + l.sinb1 * sinb + l.cosb1 * cosb * coslam,
            EQUIT => b = 1.0 + cosb * coslam,
            N_POLE => {
                b = HALF_PI + phi;
                q = l.qp - q;
            }
            S_POLE => {
                b = phi - HALF_PI;
                q = l.qp + q;
            }
            _ => {}
        }
        if abs(b) < EPSLN {
            return Out::Null;
        }
        match l.mode {
            OBLIQ | EQUIT => {
                b = sqrt(2.0 / b);
                if l.mode == OBLIQ {
                    y = l.ymf * b * (l.cosb1 * sinb - l.sinb1 * cosb * coslam);
                } else {
                    b = sqrt(2.0 / (1.0 + cosb * coslam));
                    y = b * sinb * l.ymf;
                }
                x = l.xmf * b * cosb * sinlam;
            }
            N_POLE | S_POLE => {
                if q >= 0.0 {
                    b = sqrt(q);
                    x = b * sinlam;
                    y = coslam * (if l.mode == S_POLE { b } else { -b });
                } else {
                    x = 0.0;
                    y = 0.0;
                }
            }
            _ => {}
        }
    }
    pt.x = p.a * x + n(p.x0);
    pt.y = p.a * y + n(p.y0);
    Out::P(pt)
}

/// A coefficient of the NZMG series (constant tables, constant indices).
fn nz(a: &[f64], k: usize) -> f64 {
    a.get(k).copied().unwrap_or(f64::NAN)
}

fn nzmg_forward(p: &Proj, mut pt: Pt) -> Out {
    let delta_lat = pt.y - n(p.lat0);
    let delta_lon = pt.x - n(p.long0);
    let d_phi = delta_lat / SEC_TO_RAD * 1E-5;
    let d_lambda = delta_lon;
    let mut d_phi_n = 1.0;
    let mut d_psi = 0.0;
    for k in 1..=10 {
        d_phi_n *= d_phi;
        d_psi += nz(&NZ_A, k) * d_phi_n;
    }
    let th_re = d_psi;
    let th_im = d_lambda;
    let (mut th_n_re, mut th_n_im) = (1.0, 0.0);
    let (mut z_re, mut z_im) = (0.0, 0.0);
    for k in 1..=6 {
        let r1 = th_n_re * th_re - th_n_im * th_im;
        let i1 = th_n_im * th_re + th_n_re * th_im;
        th_n_re = r1;
        th_n_im = i1;
        z_re = z_re + nz(&NZ_B_RE, k) * th_n_re - nz(&NZ_B_IM, k) * th_n_im;
        z_im = z_im + nz(&NZ_B_IM, k) * th_n_re + nz(&NZ_B_RE, k) * th_n_im;
    }
    pt.x = (z_im * p.a) + n(p.x0);
    pt.y = (z_re * p.a) + n(p.y0);
    Out::P(pt)
}

const NZ_A: [f64; 11] =
    [0.0, 0.6399175073, -0.1358797613, 0.063294409, -0.02526853, 0.0117879, -0.0055161, 0.0026906, -0.001333, 0.00067, -0.00034];
const NZ_B_RE: [f64; 7] = [0.0, 0.7557853228, 0.249204646, -0.001541739, -0.10162907, -0.26623489, -0.6870983];
const NZ_B_IM: [f64; 7] = [0.0, 0.0, 0.003371507, 0.041058560, 0.01727609, -0.36249218, -1.1651967];
const NZ_C_RE: [f64; 7] = [0.0, 1.3231270439, -0.577245789, 0.508307513, -0.15094762, 1.01418179, 1.9660549];
const NZ_C_IM: [f64; 7] = [0.0, 0.0, -0.007809598, -0.112208952, 0.18200602, 1.64497696, 2.5127645];
const NZ_D: [f64; 10] = [0.0, 1.5627014243, 0.5185406398, -0.03333098, -0.1052906, -0.0368594, 0.007317, 0.01220, 0.00394, -0.0013];

fn aeqd_forward(p: &Proj, a: &Aeqd, mut pt: Pt) -> Out {
    let long0 = n(p.long0);
    let lat0 = n(p.lat0);
    let (x0, y0) = (n(p.x0), n(p.y0));
    let lon = pt.x;
    let lat = pt.y;
    let sinphi = sin(pt.y);
    let cosphi = cos(pt.y);
    let dlon = adjust_lon(lon - long0, p.over);
    if p.sphere {
        if abs(a.sin_p12 - 1.0) <= EPSLN {
            pt.x = x0 + p.a * (HALF_PI - lat) * sin(dlon);
            pt.y = y0 - p.a * (HALF_PI - lat) * cos(dlon);
        } else if abs(a.sin_p12 + 1.0) <= EPSLN {
            pt.x = x0 + p.a * (HALF_PI + lat) * sin(dlon);
            pt.y = y0 + p.a * (HALF_PI + lat) * cos(dlon);
        } else {
            let cos_c = a.sin_p12 * sinphi + a.cos_p12 * cosphi * cos(dlon);
            let c = acos(cos_c);
            let kp = if c != 0.0 && !c.is_nan() { c / sin(c) } else { 1.0 };
            pt.x = x0 + p.a * kp * cosphi * sin(dlon);
            pt.y = y0 + p.a * kp * (a.cos_p12 * sinphi - a.sin_p12 * cosphi * cos(dlon));
        }
        return Out::P(pt);
    }
    let e0 = e0fn(p.es);
    let e1 = e1fn(p.es);
    let e2 = e2fn(p.es);
    let e3 = e3fn(p.es);
    if abs(a.sin_p12 - 1.0) <= EPSLN {
        let mlp = p.a * mlfn(e0, e1, e2, e3, HALF_PI);
        let ml = p.a * mlfn(e0, e1, e2, e3, lat);
        pt.x = x0 + (mlp - ml) * sin(dlon);
        pt.y = y0 - (mlp - ml) * cos(dlon);
    } else if abs(a.sin_p12 + 1.0) <= EPSLN {
        let mlp = p.a * mlfn(e0, e1, e2, e3, HALF_PI);
        let ml = p.a * mlfn(e0, e1, e2, e3, lat);
        pt.x = x0 + (mlp + ml) * sin(dlon);
        pt.y = y0 + (mlp + ml) * cos(dlon);
    } else {
        if abs(lon) < EPSLN && abs(lat - lat0) < EPSLN {
            pt.x = x0;
            pt.y = y0;
            return Out::P(pt);
        }
        let (azi1, s12) = vincenty_inverse(lat0, long0, lat, lon, p.a, a.f);
        pt.x = x0 + s12 * sin(azi1);
        pt.y = y0 + s12 * cos(azi1);
    }
    Out::P(pt)
}

/// `inverse(p)` of the projection.
pub fn inverse(p: &Proj, mut pt: Pt) -> R<Out> {
    let long0 = n(p.long0);
    let lat0 = n(p.lat0);
    let (x0, y0) = (n(p.x0), n(p.y0));
    let k0 = n(p.k0);
    Ok(match &p.kind {
        Kind::None => return Err("p.inverse is not a function".into()),
        Kind::Longlat => Out::P(pt),
        Kind::Merc => {
            let x = pt.x - x0;
            let y = pt.y - y0;
            let lat;
            if p.sphere {
                lat = HALF_PI - 2.0 * atan(exp(-y / (p.a * k0)));
            } else {
                let ts = exp(-y / (p.a * k0));
                lat = phi2z(p.e, ts);
                if lat == -9999.0 {
                    return Ok(Out::Null);
                }
            }
            let lon = adjust_lon(long0 + x / (p.a * k0), p.over);
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Tmerc(t) => tmerc_inverse(p, t, pt),
        Kind::Etmerc(st) => {
            if let Some(t) = &st.approx {
                return Ok(tmerc_inverse(p, t, pt));
            }
            let mut ce = (pt.x - x0) * (1.0 / p.a);
            let mut cn = (pt.y - y0) * (1.0 / p.a);
            cn = (cn - st.zb) / st.qn;
            ce /= st.qn;
            let (lon, lat);
            if abs(ce) <= 2.623395162778 {
                let tmp = clens_cmplx(&st.utg, 2.0 * cn, 2.0 * ce);
                cn += tmp[0];
                ce += tmp[1];
                ce = atan(sinh(ce));
                let sin_cn = sin(cn);
                let cos_cn = cos(cn);
                let sin_ce = sin(ce);
                let cos_ce = cos(ce);
                cn = atan2(sin_cn * cos_ce, hypot(sin_ce, cos_ce * cos_cn));
                ce = atan2(sin_ce, cos_ce * cos_cn);
                lon = adjust_lon(ce + long0, p.over);
                lat = gatg(&st.cgb, cn);
            } else {
                lon = f64::INFINITY;
                lat = f64::INFINITY;
            }
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Sterea(s) => {
            pt.x = (pt.x - x0) / p.a;
            pt.y = (pt.y - y0) / p.a;
            pt.x /= k0;
            pt.y /= k0;
            let rho = hypot(pt.x, pt.y);
            let (lat, lon);
            if rho != 0.0 && !rho.is_nan() {
                let c = 2.0 * atan2(rho, s.r2);
                let sinc = sin(c);
                let cosc = cos(c);
                lat = asin(cosc * s.sinc0 + pt.y * sinc * s.cosc0 / rho);
                lon = atan2(pt.x * sinc, rho * s.cosc0 * cosc - pt.y * s.sinc0 * sinc);
            } else {
                lat = s.g.phic0;
                lon = 0.0;
            }
            pt.x = lon;
            pt.y = lat;
            if !gauss_inverse(&s.g, p.e, &mut pt) {
                // gauss.inverse returned null; sterea ignores it and goes on with the point as it stands
            }
            pt.x = adjust_lon(pt.x + long0, p.over);
            Out::P(pt)
        }
        Kind::Stere(s) => {
            pt.x -= x0;
            pt.y -= y0;
            let rh = sqrt(pt.x * pt.x + pt.y * pt.y);
            let (lon, lat);
            if p.sphere {
                let c = 2.0 * atan(rh / (2.0 * p.a * k0));
                if rh <= EPSLN {
                    pt.x = long0;
                    pt.y = lat0;
                    return Ok(Out::P(pt));
                }
                lat = asin(cos(c) * s.sinlat0 + pt.y * sin(c) * s.coslat0 / rh);
                if abs(s.coslat0) < EPSLN {
                    lon = if lat0 > 0.0 {
                        adjust_lon(long0 + atan2(pt.x, -1.0 * pt.y), p.over)
                    } else {
                        adjust_lon(long0 + atan2(pt.x, pt.y), p.over)
                    };
                } else {
                    lon = adjust_lon(long0 + atan2(pt.x * sin(c), rh * s.coslat0 * cos(c) - pt.y * s.sinlat0 * sin(c)), p.over);
                }
                pt.x = lon;
                pt.y = lat;
                return Ok(Out::P(pt));
            }
            if abs(s.coslat0) <= EPSLN {
                if rh <= EPSLN {
                    pt.x = long0;
                    pt.y = lat0;
                    return Ok(Out::P(pt));
                }
                pt.x *= s.con;
                pt.y *= s.con;
                let ts = rh * s.cons / (2.0 * p.a * k0);
                lat = s.con * phi2z(p.e, ts);
                lon = s.con * adjust_lon(s.con * long0 + atan2(pt.x, -1.0 * pt.y), p.over);
            } else {
                let ce = 2.0 * atan(rh * s.cos_x0 / (2.0 * p.a * k0 * s.ms1));
                let mut lo = long0;
                let chi;
                if rh <= EPSLN {
                    chi = s.x0_;
                } else {
                    chi = asin(cos(ce) * s.sin_x0 + pt.y * sin(ce) * s.cos_x0 / rh);
                    lo = adjust_lon(long0 + atan2(pt.x * sin(ce), rh * s.cos_x0 * cos(ce) - pt.y * s.sin_x0 * sin(ce)), p.over);
                }
                lon = lo;
                lat = -1.0 * phi2z(p.e, tan(0.5 * (HALF_PI + chi)));
            }
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Somerc(s) => {
            let yy = pt.x - x0;
            let xx = pt.y - y0;
            let rot_i = yy / s.r;
            let rot_b = 2.0 * (atan(exp(xx / s.r)) - PI / 4.0);
            let b = asin(cos(s.b0) * sin(rot_b) + sin(s.b0) * cos(rot_b) * cos(rot_i));
            let i = atan(sin(rot_i) / (cos(s.b0) * cos(rot_i) - sin(s.b0) * tan(rot_b)));
            let lambda = s.lambda0 + i / s.alpha;
            let mut phy = b;
            let mut prev = -1000.0;
            let mut iteration = 0;
            while abs(phy - prev) > 0.0000001 {
                iteration += 1;
                if iteration > 20 {
                    return Ok(Out::Undef);
                }
                let ss = 1.0 / s.alpha * (log(tan(PI / 4.0 + b / 2.0)) - s.k) + p.e * log(tan(PI / 4.0 + asin(p.e * sin(phy)) / 2.0));
                prev = phy;
                phy = 2.0 * atan(exp(ss)) - PI / 2.0;
            }
            pt.x = lambda;
            pt.y = phy;
            Out::P(pt)
        }
        Kind::Omerc(st) => {
            let mut c = Pt { x: f64::NAN, y: f64::NAN, z: None, m: None };
            pt.x = (pt.x - x0) * (1.0 / p.a);
            pt.y = (pt.y - y0) * (1.0 / p.a);
            let (u, v);
            if st.no_rot {
                v = pt.y;
                u = pt.x;
            } else {
                v = pt.x * st.cosrot - pt.y * st.sinrot;
                u = pt.y * st.cosrot + pt.x * st.sinrot + st.u_0;
            }
            let qp = exp(-st.br_a * v);
            let sp = 0.5 * (qp - 1.0 / qp);
            let tp = 0.5 * (qp + 1.0 / qp);
            let vp = sin(st.br_a * u);
            let up = (vp * st.cosgam + sp * st.singam) / tp;
            if abs(abs(up) - 1.0) < EPSLN {
                c.x = 0.0;
                c.y = if up < 0.0 { -HALF_PI } else { HALF_PI };
            } else {
                c.y = st.e_ / sqrt((1.0 + up) / (1.0 - up));
                c.y = phi2z(p.e, pow(c.y, 1.0 / st.b_));
                if c.y == f64::INFINITY {
                    return Err(String::new());
                }
                c.x = -st.r_b * atan2(sp * st.cosgam - vp * st.singam, cos(st.br_a * u));
            }
            c.x += st.lam0;
            c.z = pt.z;
            c.m = pt.m;
            Out::P(c)
        }
        Kind::Lcc(l) => {
            let x = (pt.x - x0) / k0;
            let y = l.rh - (pt.y - y0) / k0;
            let (rh1, mut con) = if l.ns > 0.0 { (sqrt(x * x + y * y), 1.0) } else { (-sqrt(x * x + y * y), -1.0) };
            let mut theta = 0.0;
            if rh1 != 0.0 {
                theta = atan2(con * x, con * y);
            }
            let lat;
            if rh1 != 0.0 || l.ns > 0.0 {
                con = 1.0 / l.ns;
                let ts = pow(rh1 / (p.a * l.f0), con);
                lat = phi2z(p.e, ts);
                if lat == -9999.0 {
                    return Ok(Out::Null);
                }
            } else {
                lat = -HALF_PI;
            }
            let lon = adjust_lon(theta / l.ns + long0, p.over);
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Krovak(k) => {
            std::mem::swap(&mut pt.x, &mut pt.y);
            if !p.czech {
                pt.y *= -1.0;
                pt.x *= -1.0;
            }
            let ro = sqrt(pt.x * pt.x + pt.y * pt.y);
            let eps = atan2(pt.y, pt.x);
            let d = eps / sin(k.s0);
            let s = 2.0 * (atan(pow(k.ro0 / ro, 1.0 / k.n_) * tan(k.s0 / 2.0 + k.s45)) - k.s45);
            let u = asin(cos(k.ad) * sin(s) - sin(k.ad) * cos(s) * cos(d));
            let deltav = asin(cos(s) * sin(d) / cos(u));
            pt.x = long0 - deltav / k.alfa;
            let mut fi1 = u;
            let mut ok = false;
            let mut iter = 0;
            loop {
                pt.y = 2.0
                    * (atan(
                        pow(k.k, -1.0 / k.alfa)
                            * pow(tan(u / 2.0 + k.s45), 1.0 / k.alfa)
                            * pow((1.0 + p.e * sin(fi1)) / (1.0 - p.e * sin(fi1)), p.e / 2.0),
                    ) - k.s45);
                if abs(fi1 - pt.y) < 0.0000000001 {
                    ok = true;
                }
                fi1 = pt.y;
                iter += 1;
                if ok || iter >= 15 {
                    break;
                }
            }
            if iter >= 15 {
                return Ok(Out::Null);
            }
            Out::P(pt)
        }
        Kind::Cass(c) => {
            pt.x -= x0;
            pt.y -= y0;
            let x = pt.x / p.a;
            let y = pt.y / p.a;
            let (phi, lam);
            if p.sphere {
                let dd = y + lat0;
                phi = asin(sin(dd) * cos(x));
                lam = atan2(tan(x), cos(dd));
            } else {
                let ml1 = c.ml0 / p.a + y;
                let phi1 = imlfn(ml1, c.e0, c.e1, c.e2, c.e3);
                if abs(abs(phi1) - HALF_PI) <= EPSLN {
                    pt.x = long0;
                    pt.y = HALF_PI;
                    if y < 0.0 {
                        pt.y *= -1.0;
                    }
                    return Ok(Out::P(pt));
                }
                let nl1 = g_n(p.a, p.e, sin(phi1));
                let rl1 = nl1 * nl1 * nl1 / p.a / p.a * (1.0 - p.es);
                let tl1 = pow(tan(phi1), 2.0);
                let dl = x * p.a / nl1;
                let dsq = dl * dl;
                phi = phi1 - nl1 * tan(phi1) / rl1 * dl * dl * (0.5 - (1.0 + 3.0 * tl1) * dl * dl / 24.0);
                lam = dl * (1.0 - dsq * (tl1 / 3.0 + (1.0 + 3.0 * tl1) * tl1 * dsq / 15.0)) / cos(phi1);
            }
            pt.x = adjust_lon(lam + long0, p.over);
            pt.y = adjust_lat(phi);
            Out::P(pt)
        }
        Kind::Laea(l) => return Ok(laea_inverse(p, l, pt)),
        Kind::Aea(a) => {
            pt.x -= x0;
            pt.y = a.rh - pt.y + y0;
            let (rh1, mut con) =
                if a.ns0 >= 0.0 { (sqrt(pt.x * pt.x + pt.y * pt.y), 1.0) } else { (-sqrt(pt.x * pt.x + pt.y * pt.y), -1.0) };
            let mut theta = 0.0;
            if rh1 != 0.0 {
                theta = atan2(con * pt.x, con * pt.y);
            }
            con = rh1 * a.ns0 / p.a;
            let lat = if p.sphere {
                asin((a.c - con * con) / (2.0 * a.ns0))
            } else {
                let qs = (a.c - con * con) / a.ns0;
                // phi1z → null is 0 in the arithmetic that follows (null * x = 0)
                phi1z(a.e3, qs).unwrap_or(0.0)
            };
            let lon = adjust_lon(theta / a.ns0 + long0, p.over);
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Gnom(g) => {
            pt.x = (pt.x - x0) / p.a;
            pt.y = (pt.y - y0) / p.a;
            pt.x /= k0;
            pt.y /= k0;
            let rh = sqrt(pt.x * pt.x + pt.y * pt.y);
            let (lat, lon);
            if rh != 0.0 && !rh.is_nan() {
                let c = atan2(rh, g.rc);
                let sinc = sin(c);
                let cosc = cos(c);
                lat = asinz(cosc * g.sin_p14 + (pt.y * sinc * g.cos_p14) / rh);
                let l = atan2(pt.x * sinc, rh * g.cos_p14 * cosc - pt.y * g.sin_p14 * sinc);
                lon = adjust_lon(long0 + l, p.over);
            } else {
                lat = f64::NAN; // this.phic0 is undefined for gnom
                lon = 0.0;
            }
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Cea => {
            pt.x -= x0;
            pt.y -= y0;
            let lat_ts = n(p.lat_ts);
            let (lon, lat);
            if p.sphere {
                lon = adjust_lon(long0 + (pt.x / p.a) / cos(lat_ts), p.over);
                lat = asin((pt.y / p.a) * cos(lat_ts));
            } else {
                lat = iqsfnz(p.e, 2.0 * pt.y * k0 / p.a);
                lon = adjust_lon(long0 + pt.x / (p.a * k0), p.over);
            }
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Eqc(e) => {
            let x = pt.x;
            let y = pt.y;
            pt.x = adjust_lon(long0 + ((x - x0) / (p.a * e.rc)), p.over);
            pt.y = adjust_lat(lat0 + ((y - y0) / (p.a)));
            Out::P(pt)
        }
        Kind::Poly(q) => return Ok(poly_inverse(p, q, pt)),
        Kind::Nzmg => return Ok(nzmg_inverse(p, pt)),
        Kind::Mill => {
            pt.x -= x0;
            pt.y -= y0;
            let lon = adjust_lon(long0 + pt.x / p.a, p.over);
            let lat = 2.5 * (atan(exp(0.8 * pt.y / p.a)) - PI / 4.0);
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Sinu(s) => {
            pt.x -= x0;
            let mut lon = pt.x / p.a;
            pt.y -= y0;
            let mut lat = pt.y / p.a;
            if p.sphere {
                lat /= s.c_y;
                lon /= s.c_x * (s.m + cos(lat));
                if s.m != 0.0 && !s.m.is_nan() {
                    lat = asinz((s.m * lat + sin(lat)) / s.n_);
                } else if s.n_ != 1.0 {
                    lat = asinz(sin(lat) / s.n_);
                }
                lon = adjust_lon(lon + long0, p.over);
                lat = adjust_lat(lat);
            } else {
                lat = pj_inv_mlfn(pt.y / p.a, p.es, &s.en);
                let ss = abs(lat);
                if ss < HALF_PI {
                    let sl = sin(lat);
                    let temp = long0 + pt.x * sqrt(1.0 - p.es * sl * sl) / (p.a * cos(lat));
                    lon = adjust_lon(temp, p.over);
                } else if (ss - EPSLN) < HALF_PI {
                    lon = long0;
                }
            }
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Moll => {
            pt.x -= x0;
            pt.y -= y0;
            let mut arg = pt.y / (1.4142135623731 * p.a);
            if abs(arg) > 0.999999999999 {
                arg = 0.999999999999;
            }
            let theta = asin(arg);
            let mut lon = adjust_lon(long0 + (pt.x / (0.900316316158 * p.a * cos(theta))), p.over);
            if lon < -PI {
                lon = -PI;
            }
            if lon > PI {
                lon = PI;
            }
            arg = (2.0 * theta + sin(2.0 * theta)) / PI;
            if abs(arg) > 1.0 {
                arg = 1.0;
            }
            let lat = asin(arg);
            pt.x = lon;
            pt.y = lat;
            Out::P(pt)
        }
        Kind::Eqdc(q) => {
            pt.x -= x0;
            pt.y = q.rh - pt.y + y0;
            let (rh1, con) = if q.ns >= 0.0 { (sqrt(pt.x * pt.x + pt.y * pt.y), 1.0) } else { (-sqrt(pt.x * pt.x + pt.y * pt.y), -1.0) };
            let mut theta = 0.0;
            if rh1 != 0.0 {
                theta = atan2(con * pt.x, con * pt.y);
            }
            if p.sphere {
                pt.x = adjust_lon(long0 + theta / q.ns, p.over);
                pt.y = adjust_lat(q.g - rh1 / p.a);
            } else {
                let ml = q.g - rh1 / p.a;
                pt.y = imlfn(ml, q.e0, q.e1, q.e2, q.e3);
                pt.x = adjust_lon(long0 + theta / q.ns, p.over);
            }
            Out::P(pt)
        }
        Kind::Aeqd(a) => return Ok(aeqd_inverse(p, a, pt)),
        Kind::Eqearth(q) => {
            const A1: f64 = 1.340264;
            const A2: f64 = -0.081106;
            const A3: f64 = 0.000893;
            const A4: f64 = 0.003796;
            let m = sqrt(3.0) / 2.0;
            pt.x = (pt.x - x0) / p.a;
            pt.y = (pt.y - y0) / p.a;
            if p.es != 0.0 {
                pt.x /= q.rqda;
                pt.y /= q.rqda;
            }
            let mut pl = pt.y;
            for _ in 0..12 {
                let pl2 = pl * pl;
                let pl6 = pl2 * pl2 * pl2;
                let fy = pl * (A1 + A2 * pl2 + pl6 * (A3 + A4 * pl2)) - pt.y;
                let fpy = A1 + 3.0 * A2 * pl2 + pl6 * (7.0 * A3 + 9.0 * A4 * pl2);
                let dlat = fy / fpy;
                pl -= dlat;
                if abs(dlat) < 1e-9 {
                    break;
                }
            }
            let pl2 = pl * pl;
            let pl6 = pl2 * pl2 * pl2;
            pt.x = m * pt.x * (A1 + 3.0 * A2 * pl2 + pl6 * (7.0 * A3 + 9.0 * A4 * pl2)) / cos(pl);
            pt.y = asin(sin(pl) / m);
            if p.es != 0.0 {
                pt.y = authlat(pt.y, &q.apa);
            }
            pt.x = adjust_lon(pt.x + long0, p.over);
            Out::P(pt)
        }
    })
}

/// aea's phi1z (None = the JS's null).
fn phi1z(eccent: f64, qs: f64) -> Option<f64> {
    let mut phi = asinz(0.5 * qs);
    if eccent < EPSLN {
        return Some(phi);
    }
    let eccnts = eccent * eccent;
    for _ in 1..=25 {
        let sinphi = sin(phi);
        let cosphi = cos(phi);
        let con = eccent * sinphi;
        let com = 1.0 - con * con;
        let dphi = 0.5 * com * com / cosphi * (qs / (1.0 - eccnts) - sinphi / com + 0.5 / eccent * log((1.0 - con) / (1.0 + con)));
        phi += dphi;
        if abs(dphi) <= 1e-7 {
            return Some(phi);
        }
    }
    None
}

fn laea_inverse(p: &Proj, l: &Laea, mut pt: Pt) -> Out {
    let long0 = n(p.long0);
    let lat0 = n(p.lat0);
    pt.x -= n(p.x0);
    pt.y -= n(p.y0);
    let mut x = pt.x / p.a;
    let mut y = pt.y / p.a;
    let (lam, phi);
    if p.sphere {
        let mut cosz = 0.0;
        let mut sinz = 0.0;
        let rh = sqrt(x * x + y * y);
        let mut ph = rh * 0.5;
        if ph > 1.0 {
            return Out::Null;
        }
        ph = 2.0 * asin(ph);
        if l.mode == OBLIQ || l.mode == EQUIT {
            sinz = sin(ph);
            cosz = cos(ph);
        }
        match l.mode {
            EQUIT => {
                ph = if abs(rh) <= EPSLN { 0.0 } else { asin(y * sinz / rh) };
                x *= sinz;
                y = cosz * rh;
            }
            OBLIQ => {
                ph = if abs(rh) <= EPSLN { lat0 } else { asin(cosz * l.sinph0 + y * sinz * l.cosph0 / rh) };
                x *= sinz * l.cosph0;
                y = (cosz - sin(ph) * l.sinph0) * rh;
            }
            N_POLE => {
                y = -y;
                ph = HALF_PI - ph;
            }
            S_POLE => {
                ph -= HALF_PI;
            }
            _ => {}
        }
        lam = if y == 0.0 && (l.mode == EQUIT || l.mode == OBLIQ) { 0.0 } else { atan2(x, y) };
        phi = ph;
    } else {
        let mut ab = 0.0;
        if l.mode == OBLIQ || l.mode == EQUIT {
            x /= l.dd;
            y *= l.dd;
            let rho = sqrt(x * x + y * y);
            if rho < EPSLN {
                pt.x = long0;
                pt.y = lat0;
                return Out::P(pt);
            }
            let mut s_ce = 2.0 * asin(0.5 * rho / l.rq);
            let c_ce = cos(s_ce);
            s_ce = sin(s_ce);
            x *= s_ce;
            if l.mode == OBLIQ {
                ab = c_ce * l.sinb1 + y * s_ce * l.cosb1 / rho;
                y = rho * l.cosb1 * c_ce - y * l.sinb1 * s_ce;
            } else {
                ab = y * s_ce / rho;
                y = rho * c_ce;
            }
        } else if l.mode == N_POLE || l.mode == S_POLE {
            if l.mode == N_POLE {
                y = -y;
            }
            let q = x * x + y * y;
            if q == 0.0 || q.is_nan() {
                pt.x = long0;
                pt.y = lat0;
                return Out::P(pt);
            }
            ab = 1.0 - q / l.qp;
            if l.mode == S_POLE {
                ab = -ab;
            }
        }
        lam = atan2(x, y);
        phi = authlat(asin(ab), &l.apa);
    }
    pt.x = adjust_lon(long0 + lam, p.over);
    pt.y = phi;
    Out::P(pt)
}

fn poly_inverse(p: &Proj, q: &Poly, mut pt: Pt) -> Out {
    let long0 = n(p.long0);
    let lat0 = n(p.lat0);
    let x = pt.x - n(p.x0);
    let y = pt.y - n(p.y0);
    let lon;
    let mut lat = f64::NAN;
    if p.sphere {
        if abs(y + p.a * lat0) <= EPSLN {
            lon = adjust_lon(x / p.a + long0, p.over);
            lat = 0.0;
        } else {
            let al = lat0 + y / p.a;
            let bl = x * x / p.a / p.a + al * al;
            let mut phi = al;
            for _ in 0..20 {
                let tanphi = tan(phi);
                let dphi = -1.0 * (al * (phi * tanphi + 1.0) - phi - 0.5 * (phi * phi + bl) * tanphi) / ((phi - al) / tanphi - 1.0);
                phi += dphi;
                if abs(dphi) <= EPSLN {
                    lat = phi;
                    break;
                }
            }
            lon = adjust_lon(long0 + (asin(x * tan(phi) / p.a)) / sin(lat), p.over);
        }
    } else if abs(y + q.ml0) <= EPSLN {
        lat = 0.0;
        lon = adjust_lon(long0 + x / p.a, p.over);
    } else {
        let al = (q.ml0 + y) / p.a;
        let bl = x * x / p.a / p.a + al * al;
        let mut phi = al;
        for _ in 0..20 {
            let con = p.e * sin(phi);
            let cl = sqrt(1.0 - con * con) * tan(phi);
            let mln = p.a * mlfn(q.e0, q.e1, q.e2, q.e3, phi);
            let mlnp = q.e0 - 2.0 * q.e1 * cos(2.0 * phi) + 4.0 * q.e2 * cos(4.0 * phi) - 6.0 * q.e3 * cos(6.0 * phi);
            let ma = mln / p.a;
            let dphi = (al * (cl * ma + 1.0) - ma - 0.5 * cl * (ma * ma + bl))
                / (p.es * sin(2.0 * phi) * (ma * ma + bl - 2.0 * al * ma) / (4.0 * cl) + (al - ma) * (cl * mlnp - 2.0 / sin(2.0 * phi))
                    - mlnp);
            phi -= dphi;
            if abs(dphi) <= EPSLN {
                lat = phi;
                break;
            }
        }
        let cl = sqrt(1.0 - p.es * pow(sin(lat), 2.0)) * tan(lat);
        lon = adjust_lon(long0 + asin(x * cl / p.a) / sin(lat), p.over);
    }
    pt.x = lon;
    pt.y = lat;
    Out::P(pt)
}

fn nzmg_inverse(p: &Proj, mut pt: Pt) -> Out {
    let delta_x = pt.x - n(p.x0);
    let delta_y = pt.y - n(p.y0);
    let z_re = delta_y / p.a;
    let z_im = delta_x / p.a;
    let (mut z_n_re, mut z_n_im) = (1.0, 0.0);
    let (mut th_re, mut th_im) = (0.0, 0.0);
    for k in 1..=6 {
        let r1 = z_n_re * z_re - z_n_im * z_im;
        let i1 = z_n_im * z_re + z_n_re * z_im;
        z_n_re = r1;
        z_n_im = i1;
        th_re = th_re + nz(&NZ_C_RE, k) * z_n_re - nz(&NZ_C_IM, k) * z_n_im;
        th_im = th_im + nz(&NZ_C_IM, k) * z_n_re + nz(&NZ_C_RE, k) * z_n_im;
    }
    // iterations = 1
    {
        let (mut th_n_re, mut th_n_im) = (th_re, th_im);
        let (mut num_re, mut num_im) = (z_re, z_im);
        for k in 2..=6 {
            let r1 = th_n_re * th_re - th_n_im * th_im;
            let i1 = th_n_im * th_re + th_n_re * th_im;
            th_n_re = r1;
            th_n_im = i1;
            let kf = (k - 1) as f64;
            num_re += kf * (nz(&NZ_B_RE, k) * th_n_re - nz(&NZ_B_IM, k) * th_n_im);
            num_im += kf * (nz(&NZ_B_IM, k) * th_n_re + nz(&NZ_B_RE, k) * th_n_im);
        }
        th_n_re = 1.0;
        th_n_im = 0.0;
        let mut den_re = nz(&NZ_B_RE, 1);
        let mut den_im = nz(&NZ_B_IM, 1);
        for k in 2..=6 {
            let r1 = th_n_re * th_re - th_n_im * th_im;
            let i1 = th_n_im * th_re + th_n_re * th_im;
            th_n_re = r1;
            th_n_im = i1;
            let kf = k as f64;
            den_re += kf * (nz(&NZ_B_RE, k) * th_n_re - nz(&NZ_B_IM, k) * th_n_im);
            den_im += kf * (nz(&NZ_B_IM, k) * th_n_re + nz(&NZ_B_RE, k) * th_n_im);
        }
        let den2 = den_re * den_re + den_im * den_im;
        th_re = (num_re * den_re + num_im * den_im) / den2;
        th_im = (num_im * den_re - num_re * den_im) / den2;
    }
    let d_psi = th_re;
    let d_lambda = th_im;
    let mut d_psi_n = 1.0;
    let mut d_phi = 0.0;
    for k in 1..=9 {
        d_psi_n *= d_psi;
        d_phi += nz(&NZ_D, k) * d_psi_n;
    }
    let lat = n(p.lat0) + (d_phi * SEC_TO_RAD * 1E5);
    let lon = n(p.long0) + d_lambda;
    pt.x = lon;
    pt.y = lat;
    Out::P(pt)
}

fn aeqd_inverse(p: &Proj, a: &Aeqd, mut pt: Pt) -> Out {
    let long0 = n(p.long0);
    let lat0 = n(p.lat0);
    pt.x -= n(p.x0);
    pt.y -= n(p.y0);
    if p.sphere {
        let rh = sqrt(pt.x * pt.x + pt.y * pt.y);
        if rh > (2.0 * HALF_PI * p.a) {
            return Out::Undef;
        }
        let z = rh / p.a;
        let sinz = sin(z);
        let cosz = cos(z);
        let mut lon = long0;
        let lat;
        if abs(rh) <= EPSLN {
            lat = lat0;
        } else {
            lat = asinz(cosz * a.sin_p12 + (pt.y * sinz * a.cos_p12) / rh);
            let con = abs(lat0) - HALF_PI;
            if abs(con) <= EPSLN {
                lon = if lat0 >= 0.0 {
                    adjust_lon(long0 + atan2(pt.x, -pt.y), p.over)
                } else {
                    adjust_lon(long0 - atan2(-pt.x, pt.y), p.over)
                };
            } else {
                lon = adjust_lon(long0 + atan2(pt.x * sinz, rh * a.cos_p12 * cosz - pt.y * a.sin_p12 * sinz), p.over);
            }
        }
        pt.x = lon;
        pt.y = lat;
        return Out::P(pt);
    }
    let e0 = e0fn(p.es);
    let e1 = e1fn(p.es);
    let e2 = e2fn(p.es);
    let e3 = e3fn(p.es);
    if abs(a.sin_p12 - 1.0) <= EPSLN {
        let mlp = p.a * mlfn(e0, e1, e2, e3, HALF_PI);
        let rh = sqrt(pt.x * pt.x + pt.y * pt.y);
        let m = mlp - rh;
        let lat = imlfn(m / p.a, e0, e1, e2, e3);
        let lon = adjust_lon(long0 + atan2(pt.x, -1.0 * pt.y), p.over);
        pt.x = lon;
        pt.y = lat;
    } else if abs(a.sin_p12 + 1.0) <= EPSLN {
        let mlp = p.a * mlfn(e0, e1, e2, e3, HALF_PI);
        let rh = sqrt(pt.x * pt.x + pt.y * pt.y);
        let m = rh - mlp;
        let lat = imlfn(m / p.a, e0, e1, e2, e3);
        let lon = adjust_lon(long0 + atan2(pt.x, pt.y), p.over);
        pt.x = lon;
        pt.y = lat;
    } else {
        let azi1 = atan2(pt.x, pt.y);
        let s12 = sqrt(pt.x * pt.x + pt.y * pt.y);
        let (lat2, lon2) = vincenty_direct(lat0, long0, azi1, s12, p.a, a.f);
        pt.x = lon2;
        pt.y = lat2;
    }
    Out::P(pt)
}
