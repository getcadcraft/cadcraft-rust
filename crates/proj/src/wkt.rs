//! wkt-parser (MIT, bundled in proj4js 2.22.0): WKT 1 / WKT 2 text → the definition object proj4js builds a
//! projection from. Ported one for one on [`Val`] so the odd corners (a keyword used as a key, `obj[v] = true` for a bare
//! value, the PROJJSON step of WKT 2) give the same object. Errors carry the JS's message (a TypeError where the JS
//! would throw one on malformed input).

use crate::defs::D2R;
use crate::val::{Props, Val, parse_float, parse_int};

type R<T> = Result<T, String>;

fn get(v: &Val, k: &str) -> R<Val> {
    match v {
        Val::Undef => Err(format!("Cannot read properties of undefined (reading '{k}')")),
        Val::Null => Err(format!("Cannot read properties of null (reading '{k}')")),
        other => Ok(other.at(k)),
    }
}
fn lower(v: &Val) -> R<String> {
    match v {
        Val::Str(s) => Ok(s.to_lowercase()),
        other => Err(crate::defs::type_error_call(other, "toLowerCase")),
    }
}
fn d2r(v: &Val) -> Val {
    Val::Num(v.num() * D2R)
}

// ---------------------------------------------------------------------------------------------------------------
// the WKT tokenizer (Parser)

const NEUTRAL: i32 = 1;
const KEYWORD: i32 = 2;
const NUMBER: i32 = 3;
const QUOTED: i32 = 4;
const AFTERQUOTE: i32 = 5;
const ENDED: i32 = -1;

fn is_ws(c: Option<char>) -> bool {
    c.is_some_and(cc_cad::js::is_ws)
}
fn latin(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphabetic())
}
fn keyword_char(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphabetic() || c == '8' || c == '4' || c == '_')
}
fn end_thing(c: Option<char>) -> bool {
    matches!(c, Some(',' | ']'))
}
fn digit_thing(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | 'E' | '-' | '+'))
}
fn show(c: Option<char>) -> String {
    c.map_or_else(|| "undefined".to_string(), |c| c.to_string())
}

struct Parser {
    text: Vec<char>,
    place: usize,
    root: Option<Vec<Val>>,
    stack: Vec<Vec<Val>>,
    state: i32,
    /// None = JS undefined (the parser starts with it), Some(Val::Null) = null
    word: Option<Val>,
}

impl Parser {
    fn new(text: &str) -> Parser {
        let t = cc_cad::js::trim(text);
        Parser { text: t.chars().collect(), place: 0, root: None, stack: Vec::new(), state: NEUTRAL, word: None }
    }
    fn word_str(&self) -> String {
        match &self.word {
            None => "undefined".into(),
            Some(v) => v.to_js_string(),
        }
    }
    fn read(&mut self) -> R<()> {
        let mut ch = self.text.get(self.place).copied();
        self.place = self.place.saturating_add(1);
        if self.state != QUOTED {
            while is_ws(ch) {
                if self.place >= self.text.len() {
                    return Ok(());
                }
                ch = self.text.get(self.place).copied();
                self.place = self.place.saturating_add(1);
            }
        }
        match self.state {
            NEUTRAL => self.neutral(ch),
            KEYWORD => self.keyword(ch),
            QUOTED => {
                if ch == Some('"') {
                    self.state = AFTERQUOTE;
                } else {
                    let w = self.word_str() + &show(ch);
                    self.word = Some(Val::Str(w));
                }
                Ok(())
            }
            AFTERQUOTE => self.afterquote(ch),
            NUMBER => self.number(ch),
            _ => Ok(()),
        }
    }
    fn push_current(&mut self, v: Val) -> R<()> {
        match self.stack.last_mut() {
            Some(cur) => {
                cur.push(v);
                Ok(())
            }
            None => Err("Cannot read properties of null (reading 'push')".into()),
        }
    }
    fn afterquote(&mut self, ch: Option<char>) -> R<()> {
        if ch == Some('"') {
            let w = self.word_str() + "\"";
            self.word = Some(Val::Str(w));
            self.state = QUOTED;
            return Ok(());
        }
        if end_thing(ch) {
            let w = cc_cad::js::trim(&self.word_str()).to_string();
            self.word = Some(Val::Str(w));
            return self.after_item(ch);
        }
        Err(format!("havn't handled \"{}\" in afterquote yet, index {}", show(ch), self.place))
    }
    fn after_item(&mut self, ch: Option<char>) -> R<()> {
        if ch == Some(',') {
            if self.word != Some(Val::Null) {
                let w = self.word.clone().unwrap_or(Val::Undef);
                self.push_current(w)?;
            }
            self.word = Some(Val::Null);
            self.state = NEUTRAL;
            return Ok(());
        }
        if ch == Some(']') {
            if self.word != Some(Val::Null) {
                let w = self.word.clone().unwrap_or(Val::Undef);
                self.push_current(w)?;
                self.word = Some(Val::Null);
            }
            self.state = NEUTRAL;
            match self.stack.pop() {
                None => return Err("Cannot read properties of null (reading 'push')".into()),
                Some(done) => match self.stack.last_mut() {
                    Some(parent) => parent.push(Val::Arr(done)),
                    None => {
                        self.root = Some(done);
                        self.state = ENDED;
                    }
                },
            }
        }
        Ok(())
    }
    fn number(&mut self, ch: Option<char>) -> R<()> {
        if digit_thing(ch) {
            let w = self.word_str() + &show(ch);
            self.word = Some(Val::Str(w));
            return Ok(());
        }
        if end_thing(ch) {
            let w = crate::val::parse_float_str(&self.word_str());
            self.word = Some(Val::Num(w));
            return self.after_item(ch);
        }
        Err(format!("havn't handled \"{}\" in number yet, index {}", show(ch), self.place))
    }
    fn keyword(&mut self, ch: Option<char>) -> R<()> {
        if keyword_char(ch) {
            let w = self.word_str() + &show(ch);
            self.word = Some(Val::Str(w));
            return Ok(());
        }
        if ch == Some('[') {
            let w = self.word.clone().unwrap_or(Val::Undef);
            self.stack.push(vec![w]);
            self.state = NEUTRAL;
            return Ok(());
        }
        if end_thing(ch) {
            return self.after_item(ch);
        }
        Err(format!("havn't handled \"{}\" in keyword yet, index {}", show(ch), self.place))
    }
    fn neutral(&mut self, ch: Option<char>) -> R<()> {
        if latin(ch) {
            self.word = Some(Val::Str(show(ch)));
            self.state = KEYWORD;
            return Ok(());
        }
        if ch == Some('"') {
            self.word = Some(Val::Str(String::new()));
            self.state = QUOTED;
            return Ok(());
        }
        if digit_thing(ch) {
            self.word = Some(Val::Str(show(ch)));
            self.state = NUMBER;
            return Ok(());
        }
        if end_thing(ch) {
            return self.after_item(ch);
        }
        Err(format!("havn't handled \"{}\" in neutral yet, index {}", show(ch), self.place))
    }
    fn output(mut self) -> R<Vec<Val>> {
        let mut guard = 0usize;
        while self.place < self.text.len() {
            if self.state == ENDED {
                // the JS keeps reading (and ignoring) to the end
                break;
            }
            self.read()?;
            guard = guard.saturating_add(1);
            if guard > 10_000_000 {
                break;
            }
        }
        if self.state == ENDED {
            return self.root.ok_or_else(|| "unable to parse string".to_string());
        }
        let text: String = self.text.iter().collect();
        Err(format!("unable to parse string \"{text}\". State is {}", self.state))
    }
}

/// `parseString(txt)`.
pub fn parse_string(txt: &str) -> R<Vec<Val>> {
    Parser::new(txt).output()
}

// ---------------------------------------------------------------------------------------------------------------
// sExpr / mapit: the parsed lists → a nested object

const MAX_DEPTH: usize = 200;

fn key_name(k: &Val) -> String {
    k.to_js_string()
}

fn mapit(obj: &mut Props, key: Val, mut value: Vec<Val>, depth: usize) -> R<()> {
    let mut key = Some(key);
    if matches!(key, Some(Val::Arr(_))) {
        if let Some(k) = key.take() {
            value.insert(0, k);
        }
    }
    match key {
        Some(k) if k.truthy() => {
            let mut thing = Props::new();
            for item in value {
                s_expr(item, &mut thing, depth)?;
            }
            obj.set(&key_name(&k), Val::Obj(thing));
        }
        _ => {
            for item in value {
                s_expr(item, obj, depth)?;
            }
        }
    }
    Ok(())
}

fn s_expr(v: Val, obj: &mut Props, depth: usize) -> R<()> {
    if depth > MAX_DEPTH {
        return Err("Maximum call stack size exceeded".into());
    }
    let depth = depth.saturating_add(1);
    let Val::Arr(mut v) = v else {
        obj.set(&key_name(&v), Val::Bool(true));
        return Ok(());
    };
    let mut key = if v.is_empty() { Val::Undef } else { v.remove(0) };
    if key == Val::str("PARAMETER") {
        key = if v.is_empty() { Val::Undef } else { v.remove(0) };
    }
    let kn = key_name(&key);
    if v.len() == 1 {
        let first = v.remove(0);
        if let Val::Arr(_) = first {
            let mut inner = Props::new();
            s_expr(first, &mut inner, depth)?;
            obj.set(&kn, Val::Obj(inner));
            return Ok(());
        }
        obj.set(&kn, first);
        return Ok(());
    }
    if v.is_empty() {
        obj.set(&kn, Val::Bool(true));
        return Ok(());
    }
    if kn == "TOWGS84" && matches!(key, Val::Str(_)) {
        obj.set(&kn, Val::Arr(v));
        return Ok(());
    }
    if kn == "AXIS" && matches!(key, Val::Str(_)) {
        if !obj.has(&kn) {
            obj.set(&kn, Val::Arr(Vec::new()));
        }
        match obj.get_mut(&kn) {
            Some(Val::Arr(a)) => a.push(Val::Arr(v)),
            Some(other) => return Err(crate::defs::type_error_call(other, "push")),
            None => {}
        }
        return Ok(());
    }
    let key_is_arr = matches!(key, Val::Arr(_));
    if !key_is_arr {
        obj.set(&kn, Val::Obj(Props::new()));
    }
    let skey = if let Val::Str(s) = &key { s.as_str() } else { "" };
    match skey {
        "UNIT" | "PRIMEM" | "VERT_DATUM" => {
            let name = lower(&v.first().cloned().unwrap_or(Val::Undef))?;
            let mut o = Props::new();
            o.set("name", Val::Str(name));
            o.set("convert", v.get(1).cloned().unwrap_or(Val::Undef));
            if v.len() == 3
                && let Some(x) = v.get(2).cloned()
            {
                s_expr(x, &mut o, depth)?;
            }
            obj.set(&kn, Val::Obj(o));
            Ok(())
        }
        "SPHEROID" | "ELLIPSOID" => {
            let mut o = Props::new();
            o.set("name", v.first().cloned().unwrap_or(Val::Undef));
            o.set("a", v.get(1).cloned().unwrap_or(Val::Undef));
            o.set("rf", v.get(2).cloned().unwrap_or(Val::Undef));
            if v.len() == 4
                && let Some(x) = v.get(3).cloned()
            {
                s_expr(x, &mut o, depth)?;
            }
            obj.set(&kn, Val::Obj(o));
            Ok(())
        }
        "EDATUM" | "ENGINEERINGDATUM" | "LOCAL_DATUM" | "DATUM" | "VERT_CS" | "VERTCRS" | "VERTICALCRS" => {
            let n = v.first().cloned().unwrap_or(Val::Undef);
            if let Some(slot) = v.first_mut() {
                *slot = Val::Arr(vec![Val::str("name"), n]);
            }
            mapit(obj, key, v, depth)
        }
        "COMPD_CS" | "COMPOUNDCRS" | "FITTED_CS" | "PROJECTEDCRS" | "PROJCRS" | "GEOGCS" | "GEOCCS" | "PROJCS" | "LOCAL_CS" | "GEODCRS"
        | "GEODETICCRS" | "GEODETICDATUM" | "ENGCRS" | "ENGINEERINGCRS" => {
            let n = v.first().cloned().unwrap_or(Val::Undef);
            if let Some(slot) = v.first_mut() {
                *slot = Val::Arr(vec![Val::str("name"), n]);
            }
            mapit(obj, key.clone(), v, depth)?;
            if let Some(Val::Obj(o)) = obj.get_mut(&kn) {
                o.set("type", key);
            }
            Ok(())
        }
        _ => {
            if v.iter().any(|x| !matches!(x, Val::Arr(_))) {
                // sExpr(v, obj[key]) — into the object just made (or, for an array key, obj[String(key)])
                match obj.get_mut(&kn) {
                    Some(Val::Obj(inner)) => {
                        let mut tmp = std::mem::take(inner);
                        let r = s_expr(Val::Arr(v), &mut tmp, depth);
                        if let Some(Val::Obj(slot)) = obj.get_mut(&kn) {
                            *slot = tmp;
                        }
                        r
                    }
                    Some(Val::Undef) | None => Err("Cannot create property on undefined".into()),
                    Some(_) => Err(format!("Cannot create property on {kn}")),
                }
            } else {
                mapit(obj, key, v, depth)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// cleanWKT / setPropertiesFromWkt

const KNOWN_TYPES: [&str; 11] = [
    "PROJECTEDCRS",
    "PROJCRS",
    "GEOGCS",
    "GEOCCS",
    "PROJCS",
    "LOCAL_CS",
    "GEODCRS",
    "GEODETICCRS",
    "GEODETICDATUM",
    "ENGCRS",
    "ENGINEERINGCRS",
];

fn clean_wkt(w: &mut Val, depth: usize) -> R<()> {
    if depth > MAX_DEPTH {
        return Err("Maximum call stack size exceeded".into());
    }
    match w {
        Val::Obj(o) => {
            for key in o.keys() {
                if KNOWN_TYPES.contains(&key.as_str())
                    && let Some(child) = o.get_mut(&key)
                {
                    set_properties_from_wkt(child)?;
                }
                if let Some(child) = o.get_mut(&key)
                    && child.is_object_like()
                {
                    clean_wkt(child, depth.saturating_add(1))?;
                }
            }
            Ok(())
        }
        Val::Arr(a) => {
            for child in a.iter_mut() {
                if child.is_object_like() {
                    clean_wkt(child, depth.saturating_add(1))?;
                }
            }
            Ok(())
        }
        Val::Null => Err("Cannot convert undefined or null to object".into()),
        _ => Ok(()),
    }
}

fn rename(o: &mut Props, out: &str, inn: &str, f: Option<&dyn Fn(&Props, &Val) -> Val>) {
    if !o.has(out) && o.has(inn) {
        let mut v = o.at(inn);
        if let Some(f) = f {
            v = f(o, &v);
        }
        o.set(out, v);
    }
}

fn set_properties_from_wkt(wv: &mut Val) -> R<()> {
    let Val::Obj(w) = wv else {
        return match wv {
            Val::Undef | Val::Null => Err(format!("Cannot read properties of {} (reading 'AUTHORITY')", wv.to_js_string())),
            other => Err(format!("Cannot create property 'projName' on {} '{}'", type_of(other), other.to_js_string())),
        };
    };
    let auth = w.at("AUTHORITY");
    if auth.truthy() {
        match &auth {
            Val::Obj(a) => {
                if let Some(first) = a.keys().first()
                    && !first.is_empty()
                    && a.has(first)
                {
                    let t = format!("{first}:{}", a.at(first).to_js_string());
                    w.set("title", Val::Str(t));
                }
            }
            Val::Str(s) => {
                if !s.is_empty() {
                    return Err(format!("Cannot use 'in' operator to search for '0' in {s}"));
                }
            }
            _ => {}
        }
    }
    let ty = w.at("type");
    if ty == Val::str("GEOGCS") {
        w.set("projName", Val::str("longlat"));
    } else if ty == Val::str("LOCAL_CS") {
        w.set("projName", Val::str("identity"));
        w.set("local", Val::Bool(true));
    } else {
        let p = w.at("PROJECTION");
        match &p {
            Val::Obj(po) => {
                let k = po.keys().first().cloned().map_or(Val::Undef, Val::Str);
                w.set("projName", k);
            }
            Val::Arr(a) => {
                let k = if a.is_empty() { Val::Undef } else { Val::str("0") };
                w.set("projName", k);
            }
            Val::Null => w.set("projName", Val::Undef),
            other => w.set("projName", other.clone()),
        }
    }
    if let Val::Arr(axes) = w.at("AXIS") {
        let mut order = String::new();
        for ax in &axes {
            let a0 = lower(&ax.at("0"))?;
            let a1 = lower(&ax.at("1"))?;
            if a0.contains("north") || ((a0 == "y" || a0 == "lat") && a1 == "north") {
                order.push('n');
            } else if a0.contains("south") || ((a0 == "y" || a0 == "lat") && a1 == "south") {
                order.push('s');
            } else if a0.contains("east") || ((a0 == "x" || a0 == "lon") && a1 == "east") {
                order.push('e');
            } else if a0.contains("west") || ((a0 == "x" || a0 == "lon") && a1 == "west") {
                order.push('w');
            }
        }
        if order.len() == 2 {
            order.push('u');
        }
        if order.len() == 3 {
            w.set("axis", Val::Str(order));
        }
    } else if w.at("AXIS").truthy() {
        // AXIS that is not an array: its .length is undefined, nothing is read
    }
    let unit = w.at("UNIT");
    if unit.truthy() {
        let mut units = lower(&get(&unit, "name")?)?;
        if units == "metre" {
            units = "meter".into();
        }
        w.set("units", Val::Str(units));
        let conv = get(&unit, "convert")?;
        if conv.truthy() {
            if w.at("type") == Val::str("GEOGCS") {
                let datum = w.at("DATUM");
                if datum.truthy() && get(&datum, "SPHEROID")?.truthy() {
                    let a = get(&get(&datum, "SPHEROID")?, "a")?;
                    w.set("to_meter", Val::Num(conv.num() * a.num()));
                }
            } else {
                w.set("to_meter", conv);
            }
        }
    }
    let self_geog = w.at("type") == Val::str("GEOGCS");
    let geogcs = if self_geog { Val::Obj(w.clone()) } else { w.at("GEOGCS") };
    if geogcs.truthy() {
        let primem = get(&geogcs, "PRIMEM")?;
        if primem.truthy() && get(&primem, "convert")?.truthy() {
            w.set("from_greenwich", d2r(&get(&primem, "convert")?));
        }
        let datum = get(&geogcs, "DATUM")?;
        let mut dc = if datum.truthy() { lower(&get(&datum, "name")?)? } else { lower(&get(&geogcs, "name")?)? };
        if dc.starts_with("d_") {
            dc = dc.chars().skip(2).collect();
        }
        if dc == "new_zealand_1949" {
            dc = "nzgd49".into();
        }
        if dc == "wgs_1984" || dc == "world_geodetic_system_1984" {
            if w.at("PROJECTION") == Val::str("Mercator_Auxiliary_Sphere") {
                w.set("sphere", Val::Bool(true));
            }
            dc = "wgs84".into();
        }
        if dc == "belge_1972" {
            dc = "rnb72".into();
        }
        w.set("datumCode", Val::Str(dc.clone()));
        if datum.truthy() && get(&datum, "SPHEROID")?.truthy() {
            let sph = get(&datum, "SPHEROID")?;
            let name = get(&sph, "name")?;
            let Val::Str(n) = &name else { return Err(crate::defs::type_error_call(&name, "replace")) };
            let mut e = n.replacen("_19", "", 1);
            e = replace_clarke(&e);
            if e.to_lowercase().chars().take(13).collect::<String>() == "international" {
                e = "intl".into();
            }
            w.set("ellps", Val::Str(e));
            w.set("a", get(&sph, "a")?);
            w.set("rf", Val::Num(parse_float(&get(&sph, "rf")?)));
        }
        if datum.truthy() && get(&datum, "TOWGS84")?.truthy() {
            w.set("datum_params", get(&datum, "TOWGS84")?);
        }
        let mut dc = w.at("datumCode").to_js_string();
        if dc.contains("osgb_1936") {
            dc = "osgb36".into();
        }
        if dc.contains("osni_1952") {
            dc = "osni52".into();
        }
        if dc.contains("tm65") || dc.contains("geodetic_datum_of_1965") {
            dc = "ire65".into();
        }
        if dc == "ch1903+" {
            dc = "ch1903".into();
        }
        if dc.contains("israel") {
            dc = "isr93".into();
        }
        w.set("datumCode", Val::Str(dc));
    }
    let b = w.at("b");
    if b.truthy() && !b.num().is_finite() {
        let a = w.at("a");
        w.set("b", a);
    }
    let rga = w.at("rectified_grid_angle");
    if rga.truthy() {
        w.set("rectified_grid_angle", d2r(&rga));
    }
    let to_meter = |o: &Props, v: &Val| -> Val {
        let tm = o.at("to_meter");
        let ratio = if tm.truthy() { tm.num() } else { 1.0 };
        Val::Num(v.num() * ratio)
    };
    let dr = |_: &Props, v: &Val| d2r(v);
    let list: [(&str, &str, u8); 36] = [
        ("standard_parallel_1", "Standard_Parallel_1", 0),
        ("standard_parallel_1", "Latitude of 1st standard parallel", 0),
        ("standard_parallel_2", "Standard_Parallel_2", 0),
        ("standard_parallel_2", "Latitude of 2nd standard parallel", 0),
        ("false_easting", "False_Easting", 0),
        ("false_easting", "False easting", 0),
        ("false-easting", "Easting at false origin", 0),
        ("false_northing", "False_Northing", 0),
        ("false_northing", "False northing", 0),
        ("false_northing", "Northing at false origin", 0),
        ("central_meridian", "Central_Meridian", 0),
        ("central_meridian", "Longitude of natural origin", 0),
        ("central_meridian", "Longitude of false origin", 0),
        ("latitude_of_origin", "Latitude_Of_Origin", 0),
        ("latitude_of_origin", "Central_Parallel", 0),
        ("latitude_of_origin", "Latitude of natural origin", 0),
        ("latitude_of_origin", "Latitude of false origin", 0),
        ("scale_factor", "Scale_Factor", 0),
        ("k0", "scale_factor", 0),
        ("latitude_of_center", "Latitude_Of_Center", 0),
        ("latitude_of_center", "Latitude_of_center", 0),
        ("lat0", "latitude_of_center", 1),
        ("longitude_of_center", "Longitude_Of_Center", 0),
        ("longitude_of_center", "Longitude_of_center", 0),
        ("longc", "longitude_of_center", 1),
        ("x0", "false_easting", 2),
        ("y0", "false_northing", 2),
        ("long0", "central_meridian", 1),
        ("lat0", "latitude_of_origin", 1),
        ("lat0", "standard_parallel_1", 1),
        ("lat1", "standard_parallel_1", 1),
        ("lat2", "standard_parallel_2", 1),
        ("azimuth", "Azimuth", 0),
        ("alpha", "azimuth", 1),
        ("srsCode", "name", 0),
        ("", "", 3),
    ];
    for (out, inn, f) in list {
        match f {
            0 => rename(w, out, inn, None),
            1 => rename(w, out, inn, Some(&dr)),
            2 => rename(w, out, inn, Some(&to_meter)),
            _ => {}
        }
    }
    apply_projection_defaults(w)
}

fn type_of(v: &Val) -> &'static str {
    match v {
        Val::Num(_) => "number",
        Val::Bool(_) => "boolean",
        Val::Str(_) => "string",
        _ => "object",
    }
}

/// `.replace(/[Cc]larke\_18/, 'clrk')` (the first match).
fn replace_clarke(s: &str) -> String {
    let a = s.find("Clarke_18");
    let b = s.find("clarke_18");
    let at = match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    };
    match at {
        Some(i) => format!("{}clrk{}", s.get(..i).unwrap_or(""), s.get(i.saturating_add(9)..).unwrap_or("")),
        None => s.to_string(),
    }
}

fn apply_projection_defaults(w: &mut Props) -> R<()> {
    let pn = w.at("projName");
    let pn_s = if pn.truthy() { lower(&pn)? } else { String::new() };
    let normalized = pn_s.replace('_', " ");
    if w.at("long0").is_undef() && !w.at("longc").is_undef() {
        let lc = w.at("longc");
        w.set("long0", lc);
    }
    let lat_ts = w.at("lat_ts");
    let lat1 = w.at("lat1");
    let lat0 = w.at("lat0");
    if !lat_ts.truthy() && lat1.truthy() && (normalized == "stereographic south pole" || normalized == "polar stereographic (variant b)") {
        w.set("lat0", Val::Num((if lat1.num() > 0.0 { 90.0 } else { -90.0 }) * D2R));
        w.set("lat_ts", lat1);
        w.delete("lat1");
    } else if !lat_ts.truthy() && lat0.truthy() && (normalized == "polar stereographic" || normalized == "polar stereographic (variant a)")
    {
        w.set("lat_ts", lat0.clone());
        w.set("lat0", Val::Num((if lat0.num() > 0.0 { 90.0 } else { -90.0 }) * D2R));
        w.delete("lat1");
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// WKT 2: the parsed lists → PROJJSON (PROJJSONBuilderBase / 2015 / 2019) → the definition (transformPROJJSON)

fn is_kw(v: &Val, k: &str) -> bool {
    matches!(v, Val::Arr(a) if a.first() == Some(&Val::str(k)))
}
fn find<'a>(node: &'a [Val], k: &str) -> Option<&'a Vec<Val>> {
    node.iter().find(|c| is_kw(c, k)).and_then(Val::as_arr)
}
fn find_any<'a>(node: &'a [Val], ks: &[&str]) -> Option<&'a Vec<Val>> {
    node.iter().find(|c| ks.iter().any(|k| is_kw(c, k))).and_then(Val::as_arr)
}
fn nth(node: &[Val], i: usize) -> Val {
    node.get(i).cloned().unwrap_or(Val::Undef)
}

fn get_id(node: &[Val]) -> Val {
    if let Some(id) = find(node, "ID")
        && id.len() >= 3
    {
        let mut o = Props::new();
        o.set("authority", nth(id, 1));
        o.set("code", Val::Num(parse_int(&nth(id, 2))));
        return Val::Obj(o);
    }
    Val::Null
}

fn convert_unit(node: Option<&Vec<Val>>, ty: &str) -> Val {
    let mut o = Props::new();
    let Some(node) = node.filter(|n| n.len() >= 3) else {
        o.set("type", Val::str(ty));
        o.set("name", Val::str("unknown"));
        o.set("conversion_factor", Val::Null);
        return Val::Obj(o);
    };
    let cf = parse_float(&nth(node, 2));
    let id = match find(node, "ID") {
        Some(idn) => {
            let mut i = Props::new();
            i.set("authority", nth(idn, 1));
            i.set("code", Val::Num(parse_int(&nth(idn, 2))));
            Val::Obj(i)
        }
        None => Val::Null,
    };
    o.set("type", Val::str(ty));
    o.set("name", nth(node, 1));
    o.set("conversion_factor", if cf != 0.0 && !cf.is_nan() { Val::Num(cf) } else { Val::Null });
    o.set("id", id);
    Val::Obj(o)
}

fn convert_axis(node: &[Val]) -> R<Val> {
    let n1 = nth(node, 1);
    let name = if n1.truthy() { n1 } else { Val::str("Unknown") };
    let name_s = name.to_js_string();
    let direction: Val;
    let chars: Vec<char> = name_s.chars().collect();
    if chars.len() == 3 && chars.first() == Some(&'(') && chars.get(2) == Some(&')') {
        let ab = chars.get(1).map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
        direction = match ab.as_str() {
            "E" => Val::str("east"),
            "N" => Val::str("north"),
            "U" => Val::str("up"),
            _ => {
                let d = nth(node, 2);
                if d.truthy() {
                    d
                } else {
                    return Err(format!("Unknown axis abbreviation: {ab}"));
                }
            }
        };
    } else {
        let d = nth(node, 2);
        direction = if d.truthy() { d } else { Val::str("unknown") };
    }
    let order = find(node, "ORDER").map_or(Val::Null, |o| Val::Num(parse_int(&nth(o, 1))));
    let unit = convert_unit(find_any(node, &["LENGTHUNIT", "ANGLEUNIT", "SCALEUNIT"]), "unit");
    let mut o = Props::new();
    o.set("name", name);
    o.set("direction", direction);
    o.set("unit", unit);
    o.set("order", order);
    Ok(Val::Obj(o))
}

fn extract_axes(node: &[Val]) -> R<Val> {
    let mut axes: Vec<Val> = Vec::new();
    for c in node.iter().filter(|c| is_kw(c, "AXIS")) {
        if let Val::Arr(a) = c {
            axes.push(convert_axis(a)?);
        }
    }
    // .sort((a, b) => (a.order || 0) - (b.order || 0)) — stable
    let key = |v: &Val| {
        let o = v.at("order");
        if o.truthy() { o.num() } else { 0.0 }
    };
    axes.sort_by(|a, b| {
        let d = key(a) - key(b);
        if d < 0.0 {
            std::cmp::Ordering::Less
        } else if d > 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    Ok(Val::Arr(axes))
}

fn base_convert(node: &[Val], result: &mut Props, v2019: bool, depth: usize) -> R<()> {
    if depth > MAX_DEPTH {
        return Err("Maximum call stack size exceeded".into());
    }
    let d = depth.saturating_add(1);
    let conv = |n: Option<&Vec<Val>>| -> R<Val> {
        match n {
            Some(n) => {
                let mut r = Props::new();
                builder_convert(n, &mut r, v2019, d)?;
                Ok(Val::Obj(r))
            }
            None => Ok(Val::Null),
        }
    };
    let k0 = nth(node, 0);
    match k0.as_str().unwrap_or("") {
        "PROJCRS" => {
            result.set("type", Val::str("ProjectedCRS"));
            result.set("name", nth(node, 1));
            result.set("base_crs", conv(find(node, "BASEGEOGCRS"))?);
            result.set("conversion", conv(find(node, "CONVERSION"))?);
            if let Some(cs) = find(node, "CS") {
                let mut c = Props::new();
                c.set("type", nth(cs, 1));
                c.set("axis", extract_axes(node)?);
                result.set("coordinate_system", Val::Obj(c));
            }
            if let Some(lu) = find(node, "LENGTHUNIT") {
                let unit = convert_unit(Some(lu), "unit");
                match result.get_mut("coordinate_system") {
                    Some(Val::Obj(c)) => c.set("unit", unit),
                    _ => return Err("Cannot set properties of undefined (setting 'unit')".into()),
                }
            }
            result.set("id", get_id(node));
        }
        "BASEGEOGCRS" | "GEOGCRS" | "GEODCRS" => {
            result.set("type", Val::str(if k0 == Val::str("GEODCRS") { "GeodeticCRS" } else { "GeographicCRS" }));
            result.set("name", nth(node, 1));
            if let Some(de) = find_any(node, &["DATUM", "ENSEMBLE"]) {
                let mut doe = Props::new();
                builder_convert(de, &mut doe, v2019, d)?;
                if let Some(pm) = find(node, "PRIMEM")
                    && nth(pm, 1) != Val::str("Greenwich")
                {
                    let mut p = Props::new();
                    p.set("name", nth(pm, 1));
                    p.set("longitude", Val::Num(parse_float(&nth(pm, 2))));
                    doe.set("prime_meridian", Val::Obj(p));
                }
                if nth(de, 0) == Val::str("ENSEMBLE") {
                    result.set("datum_ensemble", Val::Obj(doe));
                } else {
                    result.set("datum", Val::Obj(doe));
                }
            }
            let mut c = Props::new();
            c.set("type", Val::str("ellipsoidal"));
            c.set("axis", extract_axes(node)?);
            result.set("coordinate_system", Val::Obj(c));
            result.set("id", get_id(node));
        }
        "DATUM" => {
            result.set("type", Val::str("GeodeticReferenceFrame"));
            result.set("name", nth(node, 1));
            result.set("ellipsoid", conv(find(node, "ELLIPSOID"))?);
        }
        "ENSEMBLE" => {
            result.set("type", Val::str("DatumEnsemble"));
            result.set("name", nth(node, 1));
            let members: Vec<Val> = node
                .iter()
                .filter(|c| is_kw(c, "MEMBER"))
                .filter_map(Val::as_arr)
                .map(|m| {
                    let mut o = Props::new();
                    o.set("type", Val::str("DatumEnsembleMember"));
                    o.set("name", nth(m, 1));
                    o.set("id", get_id(m));
                    Val::Obj(o)
                })
                .collect();
            result.set("members", Val::Arr(members));
            if let Some(acc) = find(node, "ENSEMBLEACCURACY") {
                result.set("accuracy", Val::Num(parse_float(&nth(acc, 1))));
            }
            if let Some(el) = find(node, "ELLIPSOID") {
                result.set("ellipsoid", conv(Some(el))?);
            }
            result.set("id", get_id(node));
        }
        "ELLIPSOID" => {
            result.set("type", Val::str("Ellipsoid"));
            result.set("name", nth(node, 1));
            result.set("semi_major_axis", Val::Num(parse_float(&nth(node, 2))));
            result.set("inverse_flattening", Val::Num(parse_float(&nth(node, 3))));
            if let Some(lu) = find(node, "LENGTHUNIT") {
                builder_convert(lu, result, v2019, d)?;
            }
        }
        "CONVERSION" => {
            result.set("type", Val::str("Conversion"));
            result.set("name", nth(node, 1));
            result.set("method", conv(find(node, "METHOD"))?);
            let mut ps = Vec::new();
            for p in node.iter().filter(|c| is_kw(c, "PARAMETER")).filter_map(Val::as_arr) {
                ps.push(conv(Some(p))?);
            }
            result.set("parameters", Val::Arr(ps));
        }
        "METHOD" => {
            result.set("type", Val::str("Method"));
            result.set("name", nth(node, 1));
            result.set("id", get_id(node));
        }
        "PARAMETER" => {
            result.set("type", Val::str("Parameter"));
            result.set("name", nth(node, 1));
            result.set("value", Val::Num(parse_float(&nth(node, 2))));
            result.set("unit", convert_unit(find_any(node, &["LENGTHUNIT", "ANGLEUNIT", "SCALEUNIT"]), "unit"));
            result.set("id", get_id(node));
        }
        "BOUNDCRS" => {
            result.set("type", Val::str("BoundCRS"));
            if let Some(src) = find(node, "SOURCECRS") {
                let content = src.iter().find(|c| matches!(c, Val::Arr(_))).and_then(Val::as_arr);
                result.set("source_crs", conv(content)?);
            }
            if let Some(tgt) = find(node, "TARGETCRS") {
                let content = tgt.iter().find(|c| matches!(c, Val::Arr(_))).and_then(Val::as_arr);
                result.set("target_crs", conv(content)?);
            }
            match find(node, "ABRIDGEDTRANSFORMATION") {
                Some(t) => result.set("transformation", conv(Some(t))?),
                None => result.set("transformation", Val::Null),
            }
        }
        "ABRIDGEDTRANSFORMATION" => {
            result.set("type", Val::str("Transformation"));
            result.set("name", nth(node, 1));
            result.set("method", conv(find(node, "METHOD"))?);
            let mut ps: Vec<Val> = Vec::new();
            for p in node.iter().filter(|c| is_kw(c, "PARAMETER") || is_kw(c, "PARAMETERFILE")).filter_map(Val::as_arr) {
                if nth(p, 0) == Val::str("PARAMETER") {
                    ps.push(conv(Some(p))?);
                } else {
                    let mut o = Props::new();
                    o.set("name", nth(p, 1));
                    o.set("value", nth(p, 2));
                    let mut id = Props::new();
                    id.set("authority", Val::str("EPSG"));
                    id.set("code", Val::Num(8656.0));
                    o.set("id", Val::Obj(id));
                    ps.push(Val::Obj(o));
                }
            }
            if ps.len() == 7
                && let Some(Val::Obj(sd)) = ps.get_mut(6)
                && sd.at("name") == Val::str("Scale difference")
            {
                let v = sd.at("value").num();
                sd.set("value", Val::Num(crate::math::round((v - 1.0) * 1e12) / 1e6));
            }
            result.set("parameters", Val::Arr(ps));
            result.set("id", get_id(node));
        }
        "AXIS" => {
            if !result.at("coordinate_system").truthy() {
                let mut c = Props::new();
                c.set("type", Val::str("unspecified"));
                c.set("axis", Val::Arr(Vec::new()));
                result.set("coordinate_system", Val::Obj(c));
            }
            let ax = convert_axis(node)?;
            match result.get_mut("coordinate_system") {
                Some(Val::Obj(c)) => match c.get_mut("axis") {
                    Some(Val::Arr(a)) => a.push(ax),
                    _ => return Err("Cannot read properties of undefined (reading 'push')".into()),
                },
                _ => return Err("Cannot read properties of undefined (reading 'axis')".into()),
            }
        }
        "LENGTHUNIT" => {
            let unit = convert_unit(Some(&node.to_vec()), "LinearUnit");
            if let Some(Val::Obj(c)) = result.get_mut("coordinate_system")
                && let Some(Val::Arr(axes)) = c.get_mut("axis")
            {
                for a in axes.iter_mut() {
                    if let Val::Obj(ao) = a
                        && !ao.at("unit").truthy()
                    {
                        ao.set("unit", unit.clone());
                    }
                }
            }
            let cf = unit.at("conversion_factor");
            if cf.truthy() && cf != Val::Num(1.0) {
                let sma = result.at("semi_major_axis");
                if sma.truthy() {
                    let mut o = Props::new();
                    o.set("value", sma);
                    o.set("unit", unit);
                    result.set("semi_major_axis", Val::Obj(o));
                }
            }
        }
        _ => {
            result.set("keyword", k0);
        }
    }
    Ok(())
}

fn builder_convert(node: &[Val], result: &mut Props, v2019: bool, depth: usize) -> R<()> {
    base_convert(node, result, v2019, depth)?;
    if v2019 {
        if let Some(cs) = find(node, "CS") {
            let mut c = Props::new();
            c.set("subtype", nth(cs, 1));
            c.set("axis", extract_axes(node)?);
            result.set("coordinate_system", Val::Obj(c));
        }
        if let Some(usage) = find(node, "USAGE") {
            let mut u = Props::new();
            if let Some(s) = find(usage, "SCOPE") {
                u.set("scope", nth(s, 1));
            }
            if let Some(a) = find(usage, "AREA") {
                u.set("area", nth(a, 1));
            }
            if let Some(b) = find(usage, "BBOX") {
                u.set("bbox", Val::Arr(b.get(1..).map(<[Val]>::to_vec).unwrap_or_default()));
            }
            result.set("usage", Val::Obj(u));
        }
    } else {
        if let Some(Val::Obj(c)) = result.get("coordinate_system")
            && c.at("subtype") == Val::str("Cartesian")
        {
            result.delete("coordinate_system");
        }
        if result.at("usage").truthy() {
            result.delete("usage");
        }
    }
    Ok(())
}

fn detect_wkt2_version(root: &[Val]) -> bool {
    // true = 2019
    if root.iter().any(|c| is_kw(c, "USAGE")) {
        return true;
    }
    false
}

fn detect_wkt_version_is2(wkt: &str) -> bool {
    let n = wkt.to_uppercase();
    ["PROJCRS", "GEOGCRS", "BOUNDCRS", "VERTCRS", "LENGTHUNIT", "ANGLEUNIT", "SCALEUNIT"].iter().any(|k| n.contains(k))
}

fn to_value(v: &Val) -> f64 {
    if v.is_object_like() {
        if let Val::Null = v {
            // null.value → TypeError in JS; the caller never passes null here
            return f64::NAN;
        }
        return v.at("value").num() * v.at("unit").at("conversion_factor").num();
    }
    v.num()
}

fn calculate_ellipsoid(value: &Val, result: &mut Props) -> R<()> {
    let ell = get(value, "ellipsoid")?;
    let radius = get(&ell, "radius")?;
    if radius.truthy() {
        result.set("a", radius);
        result.set("rf", Val::Num(0.0));
    } else {
        let sma = get(&ell, "semi_major_axis")?;
        let a = to_value(&sma);
        result.set("a", if sma.is_object_like() { Val::Num(a) } else { sma.clone() });
        let inv = ell.at("inverse_flattening");
        if !inv.is_undef() {
            result.set("rf", inv);
        } else if !sma.is_undef() && !ell.at("semi_minor_axis").is_undef() {
            let ra = result.at("a").num();
            result.set("rf", Val::Num(ra / (ra - to_value(&ell.at("semi_minor_axis")))));
        }
    }
    Ok(())
}

fn process_unit(unit: &Val) -> R<(Val, Val)> {
    match unit {
        Val::Str(s) => {
            let mut u = s.to_lowercase();
            if u == "metre" {
                u = "meter".into();
            }
            let tm = if u == "meter" { Val::Num(1.0) } else { Val::Undef };
            Ok((Val::Str(u), tm))
        }
        other if other.truthy() && other.at("name").truthy() => {
            let mut u = lower(&other.at("name"))?;
            if u == "metre" {
                u = "meter".into();
            }
            Ok((Val::Str(u), other.at("conversion_factor")))
        }
        _ => Ok((Val::Null, Val::Undef)),
    }
}

fn set_or_delete(o: &mut Props, k: &str, v: Val) {
    // JS: result.k = undefined keeps the key with value undefined
    o.set(k, v);
}

fn transform_projjson(pj: &Val, result: &mut Props, depth: usize) -> R<()> {
    if depth > MAX_DEPTH {
        return Err("Maximum call stack size exceeded".into());
    }
    let Val::Obj(p) = pj else { return Ok(()) };
    if p.at("type") == Val::str("BoundCRS") {
        transform_projjson(&p.at("source_crs"), result, depth.saturating_add(1))?;
        let tr = p.at("transformation");
        if tr.truthy() {
            let m = tr.at("method");
            if m.truthy() && m.at("name") == Val::str("NTv2") {
                let ps = get(&tr, "parameters")?;
                let first = get(&ps, "0")?;
                result.set("nadgrids", get(&first, "value")?);
            } else {
                let ps = get(&tr, "parameters")?;
                let Val::Arr(list) = ps else { return Err("Cannot read properties of undefined (reading 'map')".into()) };
                let mut out = Vec::new();
                for q in &list {
                    out.push(get(q, "value")?);
                }
                result.set("datum_params", Val::Arr(out));
            }
        }
        return Ok(());
    }
    for key in p.keys() {
        let value = p.at(&key);
        if value == Val::Null {
            continue;
        }
        match key.as_str() {
            "name" => {
                if result.at("srsCode").truthy() {
                    continue;
                }
                result.set("name", value.clone());
                result.set("srsCode", value);
            }
            "type" => {
                if value == Val::str("GeographicCRS") {
                    result.set("projName", Val::str("longlat"));
                } else if value == Val::str("GeodeticCRS") {
                    let cs = p.at("coordinate_system");
                    if cs.truthy() && cs.at("subtype") == Val::str("Cartesian") {
                        result.set("projName", Val::str("geocent"));
                    } else {
                        result.set("projName", Val::str("longlat"));
                    }
                } else if value == Val::str("ProjectedCRS") && p.at("conversion").truthy() && p.at("conversion").at("method").truthy() {
                    result.set("projName", p.at("conversion").at("method").at("name"));
                }
            }
            "datum" | "datum_ensemble" => {
                if get(&value, "ellipsoid")?.truthy() {
                    result.set("ellps", get(&get(&value, "ellipsoid")?, "name")?);
                    calculate_ellipsoid(&value, result)?;
                }
                let pm = get(&value, "prime_meridian")?;
                if pm.truthy() {
                    result.set("from_greenwich", Val::Num(pm.at("longitude").num() * std::f64::consts::PI / 180.0));
                }
            }
            "ellipsoid" => {
                result.set("ellps", get(&value, "name")?);
                calculate_ellipsoid(&value, result)?;
            }
            "prime_meridian" => {
                let lon = value.at("longitude");
                let l = if lon.truthy() { lon.num() } else { 0.0 };
                result.set("long0", Val::Num(l * std::f64::consts::PI / 180.0));
            }
            "coordinate_system" => {
                let axis = value.at("axis");
                if axis.truthy() {
                    let Val::Arr(axes) = &axis else { return Err("value.axis.map is not a function".into()) };
                    let mut mapped: Vec<Option<char>> = Vec::new();
                    for a in axes {
                        let dir = lower(&get(a, "direction")?)?;
                        mapped.push(match dir.as_str() {
                            "east" | "geocentricx" => Some('e'),
                            "north" | "geocentricy" => Some('n'),
                            "west" => Some('w'),
                            "south" => Some('s'),
                            "up" | "geocentricz" => Some('u'),
                            "down" => Some('d'),
                            _ => None,
                        });
                    }
                    if mapped.iter().all(Option::is_some) {
                        let mut s: String = mapped.iter().flatten().collect();
                        if s.len() == 2 {
                            s.push('u');
                        }
                        result.set("axis", Val::Str(s));
                    }
                    let unit = value.at("unit");
                    if unit.truthy() {
                        let (u, tm) = process_unit(&unit)?;
                        set_or_delete(result, "units", u);
                        set_or_delete(result, "to_meter", tm);
                    } else if axes.first().is_some_and(|a| a.at("unit").truthy()) {
                        let (u, tm) = process_unit(&axes.first().map(|a| a.at("unit")).unwrap_or(Val::Undef))?;
                        set_or_delete(result, "units", u);
                        set_or_delete(result, "to_meter", tm);
                    }
                }
            }
            "id" => {
                if value.at("authority").truthy() && value.at("code").truthy() {
                    result.set("title", Val::Str(format!("{}:{}", value.at("authority").to_js_string(), value.at("code").to_js_string())));
                }
            }
            "conversion" => {
                let m = value.at("method");
                if m.truthy() && m.at("name").truthy() {
                    result.set("projName", m.at("name"));
                }
                if let Val::Arr(params) = value.at("parameters") {
                    for param in &params {
                        let pname = lower(&get(param, "name")?)?;
                        let pname: String = {
                            // .replace(/\s+/g, '_')
                            let mut out = String::new();
                            let mut in_ws = false;
                            for c in pname.chars() {
                                if cc_cad::js::is_ws(c) {
                                    if !in_ws {
                                        out.push('_');
                                    }
                                    in_ws = true;
                                } else {
                                    out.push(c);
                                    in_ws = false;
                                }
                            }
                            out
                        };
                        let pv = param.at("value");
                        let unit = param.at("unit");
                        if unit.truthy() && unit.at("conversion_factor").truthy() {
                            result.set(&pname, Val::Num(pv.num() * unit.at("conversion_factor").num()));
                        } else if unit == Val::str("degree") {
                            result.set(&pname, Val::Num(pv.num() * std::f64::consts::PI / 180.0));
                        } else {
                            result.set(&pname, pv);
                        }
                    }
                } else if value.at("parameters").truthy() {
                    return Err("value.parameters.forEach is not a function".into());
                }
            }
            "unit" => {
                if value.at("name").truthy() {
                    let mut u = lower(&value.at("name"))?;
                    if u == "metre" {
                        u = "meter".into();
                    }
                    result.set("units", Val::Str(u));
                }
                if value.at("conversion_factor").truthy() {
                    result.set("to_meter", value.at("conversion_factor"));
                }
            }
            "base_crs" => {
                transform_projjson(&value, result, depth.saturating_add(1))?;
                let id = value.at("id");
                let dc = if id.truthy() {
                    Val::Str(format!("{}_{}", id.at("authority").to_js_string(), id.at("code").to_js_string()))
                } else {
                    value.at("name")
                };
                result.set("datumCode", dc);
            }
            _ => {}
        }
    }
    let copy = |r: &mut Props, from: &str, to: &str, defined: bool| {
        let v = r.at(from);
        if (defined && !v.is_undef()) || (!defined && v.truthy()) {
            r.set(to, v);
        }
    };
    copy(result, "latitude_of_false_origin", "lat0", true);
    copy(result, "longitude_of_false_origin", "long0", true);
    if !result.at("latitude_of_standard_parallel").is_undef() {
        let v = result.at("latitude_of_standard_parallel");
        result.set("lat0", v.clone());
        result.set("lat1", v);
    }
    copy(result, "latitude_of_1st_standard_parallel", "lat1", true);
    copy(result, "latitude_of_2nd_standard_parallel", "lat2", true);
    copy(result, "latitude_of_projection_centre", "lat0", true);
    copy(result, "longitude_of_projection_centre", "longc", true);
    copy(result, "easting_at_false_origin", "x0", true);
    copy(result, "northing_at_false_origin", "y0", true);
    copy(result, "latitude_of_natural_origin", "lat0", true);
    copy(result, "longitude_of_natural_origin", "long0", true);
    copy(result, "longitude_of_origin", "long0", true);
    copy(result, "false_easting", "x0", true);
    copy(result, "easting_at_projection_centre", "x0", false);
    copy(result, "false_northing", "y0", true);
    copy(result, "northing_at_projection_centre", "y0", false);
    copy(result, "standard_parallel_1", "lat1", true);
    copy(result, "standard_parallel_2", "lat2", true);
    copy(result, "scale_factor_at_natural_origin", "k0", true);
    copy(result, "scale_factor_at_projection_centre", "k0", true);
    copy(result, "scale_factor_on_pseudo_standard_parallel", "k0", true);
    copy(result, "azimuth", "alpha", true);
    copy(result, "azimuth_at_projection_centre", "alpha", true);
    copy(result, "angle_from_rectified_to_skew_grid", "rectified_grid_angle", false);
    apply_projection_defaults(result)
}

/// `wkt(text)` → the definition object (Val::Obj), or whatever the JS returns (a non-object → "could not parse").
pub fn wkt(text: &str) -> R<Val> {
    let is2 = detect_wkt_version_is2(text);
    let lisp = parse_string(text)?;
    if is2 {
        let v2019 = detect_wkt2_version(&lisp);
        let mut pj = Props::new();
        builder_convert(&lisp, &mut pj, v2019, 0)?;
        let mut result = Props::new();
        transform_projjson(&Val::Obj(pj), &mut result, 0)?;
        return Ok(Val::Obj(result));
    }
    let ty = lisp.first().cloned().unwrap_or(Val::Undef).to_js_string();
    let mut obj = Props::new();
    s_expr(Val::Arr(lisp), &mut obj, 0)?;
    let mut ov = Val::Obj(obj);
    clean_wkt(&mut ov, 0)?;
    Ok(ov.at(&ty))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn esri_utm() {
        let w = r#"PROJCS["WGS_1984_UTM_Zone_15N",GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137.0,298.257223563]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Transverse_Mercator"],PARAMETER["False_Easting",500000.0],PARAMETER["False_Northing",0.0],PARAMETER["Central_Meridian",-93.0],PARAMETER["Scale_Factor",0.9996],PARAMETER["Latitude_Of_Origin",0.0],UNIT["Meter",1.0]]"#;
        let v = wkt(w).unwrap();
        assert_eq!(v.at("projName"), Val::str("Transverse_Mercator"));
        assert_eq!(v.at("datumCode"), Val::str("wgs84"));
        assert_eq!(v.at("x0"), Val::Num(500000.0));
        assert_eq!(v.at("units"), Val::str("meter"));
    }
}
