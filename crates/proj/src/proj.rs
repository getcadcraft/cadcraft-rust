//! proj4js `Proj.js` (+ `deriveConstants.js`, `datum.js`, `projections.js`): a definition → a projection with its datum.

use crate::common::{EPSLN, PJD_3PARAM, PJD_7PARAM, PJD_GRIDSHIFT, PJD_NODATUM, PJD_WGS84, RA4, RA6, SEC_TO_RAD, SIXTH};
use crate::defs::{match_datum, match_ellipsoid};
use crate::nadgrid::{GridRef, get_nadgrids};
use crate::projections::Kind;
use crate::val::{Props, Val, parse_float};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// A point `{x, y, z?, m?}`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub m: Option<f64>,
}
impl Pt {
    pub fn xy(x: f64, y: f64) -> Pt {
        Pt { x, y, z: None, m: None }
    }
}

/// What a forward / inverse returns: a point, `null`, `undefined`, or a bare number (tmerc's error code 93).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Out {
    P(Pt),
    Null,
    Undef,
    Num(f64),
}
impl Out {
    pub fn truthy(&self) -> bool {
        match self {
            Out::P(_) => true,
            Out::Num(n) => *n != 0.0 && !n.is_nan(),
            Out::Null | Out::Undef => false,
        }
    }
}

/// The datum object.
#[derive(Clone, Debug)]
pub struct Datum {
    pub datum_type: i32,
    pub datum_params: Vec<f64>,
    pub grids: Option<Vec<Option<GridRef>>>,
    pub a: f64,
    pub b: f64,
    pub es: f64,
    pub ep2: f64,
}

/// `datum(datumCode, datum_params, a, b, es, ep2, nadgrids)`.
pub fn make_datum(datum_params: Option<&Vec<Val>>, a: f64, b: f64, es: f64, ep2: f64, nadgrids: Option<Vec<Option<GridRef>>>) -> Datum {
    let mut d = Datum { datum_type: PJD_NODATUM, datum_params: Vec::new(), grids: None, a, b, es, ep2 };
    if let Some(dp) = datum_params {
        d.datum_type = PJD_WGS84;
        d.datum_params = dp.iter().map(parse_float).collect();
        let g = |i: usize| d.datum_params.get(i).copied().unwrap_or(f64::NAN);
        if g(0) != 0.0 || g(1) != 0.0 || g(2) != 0.0 {
            d.datum_type = PJD_3PARAM;
        }
        if d.datum_params.len() > 3 && (g(3) != 0.0 || g(4) != 0.0 || g(5) != 0.0 || g(6) != 0.0) {
            d.datum_type = PJD_7PARAM;
            while d.datum_params.len() < 7 {
                d.datum_params.push(f64::NAN);
            }
            if let Some(v) = d.datum_params.get_mut(3) {
                *v *= SEC_TO_RAD;
            }
            if let Some(v) = d.datum_params.get_mut(4) {
                *v *= SEC_TO_RAD;
            }
            if let Some(v) = d.datum_params.get_mut(5) {
                *v *= SEC_TO_RAD;
            }
            if let Some(v) = d.datum_params.get_mut(6) {
                *v = (*v / 1000000.0) + 1.0;
            }
        }
    }
    if nadgrids.is_some() {
        d.datum_type = PJD_GRIDSHIFT;
        d.grids = nadgrids;
    }
    d
}

/// A projection, built as proj4js builds one (`new proj4.Proj(def)`).
#[derive(Clone, Debug)]
pub struct Proj {
    /// the definition object the projection was built from (after the constructor's own edits)
    pub def: Props,
    pub proj_name: Val,
    pub title: Val,
    pub datum_code: Val,
    pub datum_name: Val,
    pub units: Val,
    pub axis: Val,
    pub ellps: Val,
    pub a: f64,
    pub b: f64,
    pub rf: f64,
    pub es: f64,
    pub e: f64,
    pub ep2: f64,
    pub sphere: bool,
    pub k0: Option<f64>,
    pub x0: Option<f64>,
    pub y0: Option<f64>,
    pub lat0: Option<f64>,
    pub lat1: Option<f64>,
    pub lat2: Option<f64>,
    pub lat_ts: Option<f64>,
    pub long0: Option<f64>,
    pub long1: Option<f64>,
    pub long2: Option<f64>,
    pub long_wrap: Option<f64>,
    pub alpha: Option<f64>,
    pub longc: Option<f64>,
    pub rectified_grid_angle: Option<f64>,
    pub zone: Option<f64>,
    pub to_meter: Option<f64>,
    pub from_greenwich: Option<f64>,
    pub k: Option<f64>,
    pub h: Option<f64>,
    pub utm_south: bool,
    pub approx: bool,
    pub over: bool,
    pub czech: bool,
    pub r_a: bool,
    /// keys present on the definition ('no_off', 'no_uoff', 'no_rot', … — `'k' in this` tests)
    pub present: Vec<String>,
    pub datum: Datum,
    pub kind: Kind,
}

fn opt_num(v: &Val) -> Option<f64> {
    match v {
        Val::Undef => None,
        other => Some(other.num()),
    }
}

/// The projection names proj4js registers (projections.js + projs.js, in order: a later name wins).
pub fn projection_id(name: &Val) -> Result<Option<usize>, String> {
    static NAMES: OnceLock<HashMap<String, usize>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let mut m = HashMap::new();
        for (i, (_, ns)) in crate::projections::REGISTRY.iter().enumerate() {
            for n in *ns {
                m.insert(n.to_lowercase(), i);
            }
        }
        m
    });
    if !name.truthy() {
        return Ok(None);
    }
    let Val::Str(n) = name else { return Err(crate::defs::type_error_call(name, "toLowerCase")) };
    let n = n.to_lowercase();
    if let Some(&i) = names.get(&n) {
        return Ok(Some(i));
    }
    let norm = normalized_proj_name(&n);
    Ok(names.get(&norm).copied())
}

/// `getNormalizedProjName(n)`: runs of `-`, `(`, `)` and white space → one space, trimmed, spaces → `_`.
pub fn normalized_proj_name(n: &str) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in n.chars() {
        if c == '-' || c == '(' || c == ')' || cc_cad::js::is_ws(c) {
            if !run {
                out.push(' ');
            }
            run = true;
        } else {
            out.push(c);
            run = false;
        }
    }
    cc_cad::js::trim(&out).replace(' ', "_")
}

impl Proj {
    /// `new Projection(srsCode)` for a definition string. Err = what the JS throws (its callback throws the message).
    pub fn new(code: &str) -> Result<Proj, String> {
        let Some(json) = crate::defs::parse(code)? else {
            return Err(format!("Could not parse to valid json: {code}"));
        };
        Proj::from_def(json, code)
    }

    pub fn from_def(mut json: Props, code: &str) -> Result<Proj, String> {
        let Some(pid) = projection_id(&json.at("projName"))? else {
            return Err(format!("Could not get projection name from: {code}"));
        };
        let dc = json.at("datumCode");
        if dc.truthy() && dc != Val::str("none") {
            if let Some(dd) = match_datum(&dc)? {
                if !json.at("datum_params").truthy() {
                    let p = match dd.towgs84 {
                        Some(t) => Val::Arr(t.split(',').map(Val::str).collect()),
                        None => Val::Null,
                    };
                    json.set("datum_params", p);
                }
                json.set("ellps", dd.ellipse.map_or(Val::Undef, Val::str));
                json.set(
                    "datumName",
                    match dd.datum_name {
                        Some(n) => Val::str(n),
                        None => dc.clone(),
                    },
                );
            }
        }
        if !json.at("axis").truthy() {
            json.set("axis", Val::str("enu"));
        }
        if !json.at("ellps").truthy() {
            json.set("ellps", Val::str("wgs84"));
        }
        let l1 = json.at("lat1");
        if !l1.truthy() {
            let l0 = json.at("lat0");
            json.set("lat1", l0);
        }

        // deriveConstants.sphere
        let mut a = json.at("a");
        let mut b = json.at("b");
        let mut rf = json.at("rf");
        if !a.truthy() {
            let e = match_ellipsoid(&json.at("ellps"))?.or_else(|| crate::tables::ELLIPSOIDS.iter().find(|e| e.key == "WGS84"));
            if let Some(e) = e {
                a = e.a.map_or(Val::Undef, Val::Num);
                b = e.b.map_or(Val::Undef, Val::Num);
                rf = e.rf.map_or(Val::Undef, Val::Num);
            }
        }
        if rf.truthy() && !b.truthy() {
            b = Val::Num((1.0 - 1.0 / rf.num()) * a.num());
        }
        let mut sphere = json.at("sphere");
        if rf == Val::Num(0.0) || (a.num() - b.num()).abs() < EPSLN {
            sphere = Val::Bool(true);
            b = a.clone();
        }
        let (af, bf, rff) = (a.num(), b.num(), rf.num());
        // deriveConstants.eccentricity
        let mut a2 = af * af;
        let b2 = bf * bf;
        let mut es = (a2 - b2) / a2;
        let mut e = 0.0;
        if json.at("R_A").truthy() {
            let a_ra = af * (1.0 - es * (SIXTH + es * (RA4 + es * RA6)));
            a2 = a_ra * a_ra;
            es = 0.0;
        } else {
            e = es.sqrt();
        }
        let ep2 = (a2 - b2) / b2;
        let nadgrids = get_nadgrids(&json.at("nadgrids"))?;
        let dp = match json.at("datum_params") {
            Val::Arr(x) => Some(x),
            Val::Undef | Val::Null => None,
            other => {
                if other.truthy() {
                    return Err(format!("{}.map is not a function", other.to_js_string()));
                }
                None
            }
        };
        let datum = make_datum(dp.as_ref(), af, bf, es, ep2, nadgrids);

        let present: Vec<String> = json.keys().into_iter().filter(|k| !json.at(k).is_undef()).collect();
        let mut p = Proj {
            proj_name: json.at("projName"),
            title: json.at("title"),
            datum_code: json.at("datumCode"),
            datum_name: json.at("datumName"),
            units: json.at("units"),
            axis: json.at("axis"),
            ellps: json.at("ellps"),
            a: af,
            b: bf,
            rf: rff,
            es,
            e,
            ep2,
            sphere: sphere.truthy(),
            k0: opt_num(&json.at("k0")),
            x0: opt_num(&json.at("x0")),
            y0: opt_num(&json.at("y0")),
            lat0: opt_num(&json.at("lat0")),
            lat1: opt_num(&json.at("lat1")),
            lat2: opt_num(&json.at("lat2")),
            lat_ts: opt_num(&json.at("lat_ts")),
            long0: opt_num(&json.at("long0")),
            long1: opt_num(&json.at("long1")),
            long2: opt_num(&json.at("long2")),
            long_wrap: opt_num(&json.at("long_wrap")),
            alpha: opt_num(&json.at("alpha")),
            longc: opt_num(&json.at("longc")),
            rectified_grid_angle: opt_num(&json.at("rectified_grid_angle")),
            zone: opt_num(&json.at("zone")),
            to_meter: opt_num(&json.at("to_meter")),
            from_greenwich: opt_num(&json.at("from_greenwich")),
            k: opt_num(&json.at("k")),
            h: opt_num(&json.at("h")),
            utm_south: json.at("utmSouth").truthy(),
            approx: json.at("approx").truthy(),
            over: json.at("over").truthy(),
            czech: json.at("czech").truthy(),
            r_a: json.at("R_A").truthy(),
            present,
            datum,
            kind: Kind::None,
            def: json,
        };
        p.kind = crate::projections::init(pid, &mut p)?;
        if !truthy(p.k0) {
            p.k0 = Some(1.0);
        }
        Ok(p)
    }

    pub fn has(&self, k: &str) -> bool {
        self.present.iter().any(|x| x == k)
    }
    pub fn forward(&self, p: Pt) -> Result<Out, String> {
        crate::projections::forward(self, p)
    }
    pub fn inverse(&self, p: Pt) -> Result<Out, String> {
        crate::projections::inverse(self, p)
    }
    /// `projName === 'longlat'`
    pub fn is_longlat(&self) -> bool {
        self.proj_name == Val::str("longlat")
    }
    pub fn is_geocent(&self) -> bool {
        self.def.at("name") == Val::str("geocent")
    }
    /// `to_meter` when truthy.
    pub fn to_meter_t(&self) -> Option<f64> {
        self.to_meter.filter(|v| *v != 0.0 && !v.is_nan())
    }
    pub fn from_greenwich_t(&self) -> Option<f64> {
        self.from_greenwich.filter(|v| *v != 0.0 && !v.is_nan())
    }
}

/// JS truthiness of an optional number.
#[inline]
pub fn truthy(v: Option<f64>) -> bool {
    v.is_some_and(|x| x != 0.0 && !x.is_nan())
}
/// The number (NaN for undefined).
#[inline]
pub fn n(v: Option<f64>) -> f64 {
    v.unwrap_or(f64::NAN)
}

/// The WGS 84 long/lat projection (`new proj('WGS84')`), built once.
pub fn wgs84() -> Arc<Proj> {
    static W: OnceLock<Option<Arc<Proj>>> = OnceLock::new();
    W.get_or_init(|| Proj::new("WGS84").ok().map(Arc::new)).clone().unwrap_or_else(|| {
        // cannot happen: the definition is a constant; build it again (and fail the same way) if it ever does
        Arc::new(Proj::new("+proj=longlat +datum=WGS84").unwrap_or_else(|_| empty_proj()))
    })
}

fn empty_proj() -> Proj {
    Proj {
        def: Props::new(),
        proj_name: Val::str("longlat"),
        title: Val::Undef,
        datum_code: Val::str("WGS84"),
        datum_name: Val::Undef,
        units: Val::Undef,
        axis: Val::str("enu"),
        ellps: Val::str("WGS84"),
        a: 6378137.0,
        b: 6356752.314245179,
        rf: 298.257223563,
        es: 0.0066943799901413165,
        e: 0.08181919084262149,
        ep2: 0.006739496742276435,
        sphere: false,
        k0: Some(1.0),
        x0: None,
        y0: None,
        lat0: None,
        lat1: None,
        lat2: None,
        lat_ts: None,
        long0: None,
        long1: None,
        long2: None,
        long_wrap: None,
        alpha: None,
        longc: None,
        rectified_grid_angle: None,
        zone: None,
        to_meter: None,
        from_greenwich: None,
        k: None,
        h: None,
        utm_south: false,
        approx: false,
        over: false,
        czech: false,
        r_a: false,
        present: Vec::new(),
        datum: Datum {
            datum_type: PJD_WGS84,
            datum_params: vec![0.0, 0.0, 0.0],
            grids: None,
            a: 6378137.0,
            b: 6356752.314245179,
            es: 0.0066943799901413165,
            ep2: 0.006739496742276435,
        },
        kind: Kind::None,
    }
}
