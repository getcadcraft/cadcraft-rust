//! The rendered picture's settings and tables: the Rust form of 0.5's `render/looks.js` (QUALITY, SIZES, SKIES, NIGHT,
//! LOOKS, MODES, MINUTES, DEFAULTS, `cleanSettings`, `lookFor`, `sizeFor`, `colorKey`, `lampCandela`), at parity with
//! the frozen JS (`forks/tools/texdiff.mjs --settings` cleans the same hostile objects on both). The renderer's
//! per-pass sequences (halton, hemisphere, sunDisc, lampAt) are in `cc-render::still`.

use serde_json::{Map, Value};

/// Passes per picture.
pub const QUALITY: [(&str, u32); 3] = [("draft", 16), ("good", 64), ("best", 256)];
/// Sizes: the view's own (×1, ×2) or fixed.
pub const SIZES: [&str; 5] = ["view", "view2", "hd", "uhd", "square"];
pub const MAX_SIDE: f64 = 4096.0;
/// Fast = the raster render; High quality = path traced.
pub const MODES: [&str; 2] = ["fast", "hq"];
/// High quality's time limits (0 = none).
pub const MINUTES: [f64; 5] = [0.0, 1.0, 5.0, 15.0, 60.0];
pub const LIGHT_SCALE: f64 = 0.02;

/// A sky: the dome's colours, the sun's and the sampled sky light's intensities, the dome's image light, the tints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    pub key: &'static str,
    pub zenith: &'static str,
    pub horizon: &'static str,
    pub ground: &'static str,
    pub sun: f64,
    pub sky: f64,
    pub env: f64,
    pub tint: &'static str,
    pub sky_tint: &'static str,
    pub plain: bool,
}

pub const SKIES: [Sky; 4] = [
    Sky {
        key: "clear",
        zenith: "#2f6cb3",
        horizon: "#c4d6e8",
        ground: "#8f8a80",
        sun: 3.2,
        sky: 1.6,
        env: 0.45,
        tint: "#fff4e5",
        sky_tint: "#dfeaff",
        plain: false,
    },
    Sky {
        key: "overcast",
        zenith: "#b9c0c8",
        horizon: "#dfe2e5",
        ground: "#8a8781",
        sun: 0.35,
        sky: 3.0,
        env: 0.8,
        tint: "#f4f2ee",
        sky_tint: "#eef0f3",
        plain: false,
    },
    Sky {
        key: "dusk",
        zenith: "#3d4f78",
        horizon: "#e9b48a",
        ground: "#5d5650",
        sun: 2.2,
        sky: 0.9,
        env: 0.35,
        tint: "#ffb877",
        sky_tint: "#b9c4e6",
        plain: false,
    },
    Sky {
        key: "white",
        zenith: "#ffffff",
        horizon: "#ffffff",
        ground: "#f2f2f2",
        sun: 2.6,
        sky: 2.0,
        env: 0.7,
        tint: "#fffaf2",
        sky_tint: "#f4f7ff",
        plain: true,
    },
];
/// The sky when the sun is down (any sky but the white studio).
pub const NIGHT: Sky = Sky {
    key: "night",
    zenith: "#070b16",
    horizon: "#18202f",
    ground: "#0c0d0f",
    sun: 0.0,
    sky: 0.05,
    env: 0.06,
    tint: "#ffffff",
    sky_tint: "#8fa3c8",
    plain: false,
};

/// A colour's look: (name, rough, metal, glass).
pub const LOOKS: [(&str, f64, f64, f64); 7] = [
    ("matte", 0.78, 0.0, 0.0),
    ("satin", 0.5, 0.0, 0.0),
    ("glossy", 0.18, 0.0, 0.0),
    ("metal", 0.32, 1.0, 0.0),
    ("chrome", 0.04, 1.0, 0.0),
    ("glass", 0.04, 0.0, 0.82),
    ("frosted", 0.45, 0.0, 0.55),
];

pub fn sky(key: &str) -> Sky {
    SKIES.iter().copied().find(|s| s.key == key).unwrap_or(SKIES[0])
}
pub fn quality(key: &str) -> u32 {
    QUALITY.iter().find(|(k, _)| *k == key).map_or(64, |(_, n)| *n)
}
pub fn look(key: &str) -> Option<(&'static str, f64, f64, f64)> {
    LOOKS.iter().copied().find(|l| l.0 == key)
}

/// The picture's settings (looks.js DEFAULTS / cleanSettings).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub mode: String,
    pub minutes: f64,
    pub size: String,
    pub quality: String,
    pub sky: String,
    pub exposure: f64,
    pub edges: bool,
    pub ground: bool,
    /// this picture's own date and time (month, day, hour), else the model's
    pub sun: Option<(f64, f64, f64)>,
    /// '#rrggbb' | 'default' → a look, in the order set
    pub looks: Vec<(String, String)>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            mode: "fast".into(),
            minutes: 5.0,
            size: "view2".into(),
            quality: "good".into(),
            sky: "clear".into(),
            exposure: 0.0,
            edges: false,
            ground: true,
            sun: None,
            looks: Vec::new(),
        }
    }
}

/// looks.js `colorKey(c)`: '#rgb' / '#rrggbb' in any case, trimmed → '#rrggbb'; else None.
pub fn color_key(c: &str) -> Option<String> {
    let s = cc_m3d::js::trim(c).to_lowercase();
    let b = s.as_bytes();
    let hex = |x: &u8| x.is_ascii_hexdigit();
    if b.len() == 4 && b.first() == Some(&b'#') && b.iter().skip(1).all(hex) {
        let ch: Vec<char> = s.chars().skip(1).collect();
        return Some(ch.iter().fold(String::from("#"), |mut o, c| {
            o.push(*c);
            o.push(*c);
            o
        }));
    }
    (b.len() == 7 && b.first() == Some(&b'#') && b.iter().skip(1).all(hex)).then_some(s)
}

fn num(v: Option<&Value>, lo: f64, hi: f64, d: f64) -> f64 {
    match v.and_then(Value::as_f64).filter(|x| x.is_finite()) {
        Some(x) => cc_m3d::js::min(hi, cc_m3d::js::max(lo, x)),
        None => d,
    }
}
fn pick(v: Option<&Value>, table: &[&str], d: &str) -> String {
    match v.and_then(Value::as_str) {
        Some(s) if table.contains(&s) => s.to_string(),
        _ => d.to_string(),
    }
}

/// looks.js `cleanSettings(obj)`: only known keys, values clamped (a file or stored settings are untrusted).
pub fn clean_settings(o: &Value) -> Settings {
    let empty = Map::new();
    let o = o.as_object().unwrap_or(&empty);
    let d = Settings::default();
    let minutes = o.get("minutes").and_then(Value::as_f64).filter(|m| MINUTES.contains(m)).unwrap_or(d.minutes);
    let mut s = Settings {
        mode: pick(o.get("mode"), &MODES, &d.mode),
        minutes,
        size: pick(o.get("size"), &SIZES, &d.size),
        quality: pick(o.get("quality"), &QUALITY.map(|q| q.0), &d.quality),
        sky: pick(o.get("sky"), &SKIES.map(|q| q.key), &d.sky),
        exposure: cc_m3d::js::round(num(o.get("exposure"), -3.0, 3.0, 0.0) * 10.0) / 10.0,
        edges: o.get("edges") == Some(&Value::Bool(true)),
        ground: o.get("ground") != Some(&Value::Bool(false)),
        sun: None,
        looks: Vec::new(),
    };
    if let Some(sun) = o.get("sun").filter(|v| v.is_object() || v.is_array()) {
        let m = cc_m3d::js::round(num(sun.get("month"), 1.0, 12.0, f64::NAN));
        let dd = cc_m3d::js::round(num(sun.get("day"), 1.0, 31.0, f64::NAN));
        let h = num(sun.get("hour"), 0.0, 24.0, f64::NAN);
        if m.is_finite() && dd.is_finite() && h.is_finite() {
            let days = [31.0, 29.0, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0];
            let lim = days.get((m as usize).saturating_sub(1)).copied().unwrap_or(31.0);
            s.sun = Some((m, dd.min(lim), cc_m3d::js::round(h * 4.0) / 4.0));
        }
    }
    if let Some(looks) = o.get("looks").and_then(Value::as_object) {
        for (k, v) in looks {
            let key = if k == "default" { Some(k.clone()) } else { color_key(k) };
            let Some(key) = key else { continue };
            let Some(v) = v.as_str().filter(|v| look(v).is_some()) else { continue };
            match s.looks.iter_mut().find(|(kk, _)| *kk == key) {
                Some(slot) => slot.1 = v.to_string(),
                None => s.looks.push((key, v.to_string())),
            }
            if s.looks.len() >= 512 {
                break;
            }
        }
    }
    s
}

impl Settings {
    /// As the JS object `cleanSettings` returns (JSON.stringify order).
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("mode".into(), Value::String(self.mode.clone()));
        o.insert("minutes".into(), cc_m3d::file::jn(self.minutes));
        o.insert("size".into(), Value::String(self.size.clone()));
        o.insert("quality".into(), Value::String(self.quality.clone()));
        o.insert("sky".into(), Value::String(self.sky.clone()));
        o.insert("exposure".into(), cc_m3d::file::jn(self.exposure));
        o.insert("edges".into(), Value::Bool(self.edges));
        o.insert("ground".into(), Value::Bool(self.ground));
        o.insert(
            "sun".into(),
            match self.sun {
                Some((m, d, h)) => {
                    let mut s = Map::new();
                    s.insert("month".into(), cc_m3d::file::jn(m));
                    s.insert("day".into(), cc_m3d::file::jn(d));
                    s.insert("hour".into(), cc_m3d::file::jn(h));
                    Value::Object(s)
                }
                None => Value::Null,
            },
        );
        let mut l = Map::new();
        for (k, v) in &self.looks {
            l.insert(k.clone(), Value::String(v.clone()));
        }
        o.insert("looks".into(), Value::Object(l));
        Value::Object(o)
    }
    /// settings.looks[key]
    pub fn look_of(&self, key: &str) -> Option<&str> {
        self.looks.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// looks.js `lookFor(key, settings, fallbackColor, ownLook)` → (colour, rough, metal, glass, look).
pub fn look_for(key: &str, s: &Settings, fallback: &str, own: Option<&str>) -> (String, f64, f64, f64, &'static str) {
    let name = s.look_of(key).or_else(|| own.filter(|o| look(o).is_some())).unwrap_or("matte");
    let l = look(name).unwrap_or(LOOKS[0]);
    let color = color_key(key).or_else(|| color_key(fallback)).unwrap_or_else(|| "#e6e3dc".into());
    (color, l.1, l.2, l.3, l.0)
}

/// looks.js `sizeFor(settings, viewW, viewH)`.
pub fn size_for(s: &Settings, vw: f64, vh: f64) -> (u32, u32) {
    let (w, h) = match s.size.as_str() {
        "view" => (cc_m3d::js::round(vw.max(1.0)), cc_m3d::js::round(vh.max(1.0))),
        "hd" => (1920.0, 1080.0),
        "uhd" => (3840.0, 2160.0),
        "square" => (2048.0, 2048.0),
        _ => (cc_m3d::js::round(vw.max(1.0) * 2.0), cc_m3d::js::round(vh.max(1.0) * 2.0)),
    };
    let k = (MAX_SIDE / w).min(MAX_SIDE / h).min(1.0);
    (cc_m3d::js::round(w * k).max(1.0) as u32, cc_m3d::js::round(h * k).max(1.0) as u32)
}

/// looks.js `lampCandela(L)`.
pub fn lamp_candela(kind: &str, lumens: f64) -> f64 {
    (if kind == "area" { lumens / std::f64::consts::PI } else { lumens / (4.0 * std::f64::consts::PI) }) * LIGHT_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_are_cleaned_as_the_js() {
        let s = clean_settings(&json!({"mode": "hq", "minutes": 7, "size": "uhd", "quality": "best", "sky": "dusk", "exposure": 2.26,
            "edges": 1, "ground": false, "sun": {"month": 2, "day": 31, "hour": 13.4}, "looks": {"#ABC": "glass", "red": "matte", "default": "chrome", "#123456": "wobble"}, "x": 1}));
        assert_eq!(s.mode, "hq");
        assert_eq!(s.minutes, 5.0);
        assert_eq!(s.exposure, 2.3);
        assert!(!s.edges && !s.ground);
        assert_eq!(s.sun, Some((2.0, 29.0, 13.5)));
        assert_eq!(s.looks, vec![("#aabbcc".to_string(), "glass".to_string()), ("default".to_string(), "chrome".to_string())]);
        assert_eq!(clean_settings(&json!(null)), Settings::default());
        assert_eq!(size_for(&Settings::default(), 1000.0, 800.0), (2000, 1600));
        assert_eq!(size_for(&Settings { size: "view2".into(), ..Settings::default() }, 3000.0, 1000.0), (4096, 1365));
    }
}
