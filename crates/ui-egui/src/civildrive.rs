//! The 3D view's Drive panel and Sun menu (drive-ui.js, 0.4 V10): drive along an alignment from a driver's seat (the
//! design profile, else the existing ground on the centerline; play / pause, a station, the speed, ← / → steps, Stop
//! puts the camera back), and light the site from where the sun is at a date and time at its latitude and longitude,
//! with shadows. The numbers are cc-civil's drive.rs (drive.js). The sun's default time zone is the computer's offset
//! from UTC now (cc_clock, as 0.5's `getTimezoneOffset`), its default date today in UTC (0.5's `toISOString`).
//! `civil.drive` / `civil.sun` drive both headless; `civil.drive.info` / `civil.sun.info` say what the panel shows.

use cc_civil::i18n::t;
use cc_civil::js::{arr, string_of};
use cc_civil::tr;
use cc_civil::units::{fixed, format_station};
use egui::RichText;
use serde_json::Value;

use crate::civilview::{CivilView, t0};
use crate::theme::Tokens;

/// Driving: the alignment, the station, playing, the camera to go back to.
#[derive(Clone, Debug)]
pub struct Drive {
    pub al: String,
    pub sta: f64,
    pub playing: bool,
    pub speed: usize,
    pub saved: Option<cc_render::mesh3d::Cam3>,
    /// the station readout (staOut)
    pub out: String,
    pub last: Option<f64>,
    /// drive.js elevationAlong's last good elevation
    pub last_z: Option<f64>,
    /// headless: only `civil.drive=tick=<s>` moves it (frames do not)
    pub manual: bool,
}

/// The Sun menu's fields (drive-ui.js readSun / applySun).
#[derive(Clone, Debug, PartialEq)]
pub struct SunSet {
    pub on: bool,
    pub shadows: bool,
    pub date: String,
    pub time: String,
    pub utc: f64,
    pub lat: String,
    pub lon: String,
}

impl SunSet {
    /// drive-ui.js `readSun()` without anything stored: today's UTC date, noon, the computer's offset now.
    pub fn defaults(now_ms: f64, zone: cc_clock::Zone) -> SunSet {
        let date = crate::civilexport::iso_time(now_ms).chars().take(10).collect();
        let mut utc = f64::from(zone.offset_min(now_ms)) / 60.0;
        if (utc * 2.0).fract() != 0.0 {
            utc = (utc * 2.0).round() / 2.0;
        }
        SunSet { on: false, shadows: true, date, time: "12:00".into(), utc, lat: String::new(), lon: String::new() }
    }

    /// The UTC time of the date and clock time (`Date.parse(date + 'T' + time + ':00Z') - utc h`).
    fn when(&self) -> Option<f64> {
        let d = self.date.as_bytes();
        let ok_date = d.len() == 10 && d.get(4) == Some(&b'-') && d.get(7) == Some(&b'-');
        if !ok_date {
            return None;
        }
        let num = |s: &str| s.parse::<i64>().ok();
        let (y, m, dd) = (num(self.date.get(0..4)?)?, num(self.date.get(5..7)?)?, num(self.date.get(8..10)?)?);
        let time = if self.time.is_empty() { "12:00" } else { self.time.as_str() };
        let (hh, mm) = time.split_once(':')?;
        let (hh, mm) = (num(hh)?, num(mm)?);
        if !(1..=12).contains(&m)
            || dd < 1
            || dd > days_in(y, m)
            || !(0..=24).contains(&hh)
            || !(0..=59).contains(&mm)
            || (hh == 24 && mm > 0)
        {
            return None;
        }
        let days = days_from_civil(y, m, dd);
        Some((days as f64) * 86_400_000.0 + (hh * 3_600_000 + mm * 60_000) as f64 - self.utc * 3_600_000.0)
    }
}

fn days_in(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl CivilView {
    fn drive_alignments(&mut self) -> Vec<Value> {
        let list: Vec<Value> = arr(self.project.state.get("alignments")).to_vec();
        list.into_iter().filter(|a| self.project.alignment(a).ok()).collect()
    }

    /// The camera at the driver's seat (drive-ui.js `place()`).
    fn drive_place(&mut self) {
        let units = self.project.units().to_string();
        let Some(d) = self.drive.clone() else { return };
        let Some(a) =
            arr(self.project.state.get("alignments")).iter().find(|a| a.get("id").and_then(Value::as_str) == Some(d.al.as_str())).cloned()
        else {
            return;
        };
        let al = self.project.alignment(&a);
        let prof = self.project.design_profile(&a);
        let eg = self.project.eg();
        // drive.js elevationAlong: the profile, else the ground on the centerline; the last good z is kept while driving
        let use_prof = prof.ok();
        let mut last = d.last_z;
        let mut elev = |sta: f64| -> Option<f64> {
            let mut z = if use_prof { prof.elevation_at(sta) } else { None };
            if !z.is_some_and(f64::is_finite) {
                z = al.point_at(sta).and_then(|p| eg.surface.as_ref().and_then(|s| s.elevation_at(p.x, p.y, 1e-6)));
            }
            if let Some(v) = z.filter(|v| v.is_finite()) {
                last = Some(v);
            }
            last
        };
        let Some(s) = self.site3.as_mut() else { return };
        let ahead = if units == "m" { 30.0 } else { 100.0 };
        let Some(f) = cc_civil::drive::drive_frame(&al, &mut elev, d.sta, cc_civil::drive::eye_height(&units), ahead, s.origin, s.exag)
        else {
            return;
        };
        // the scene is relative to its origin: the frame's eye and target already are
        s.view.cam.eye = [f.eye[0] + s.origin[0], f.eye[1] + s.origin[1], f.eye[2] + s.origin[2]];
        s.view.cam.target = [f.target[0] + s.origin[0], f.target[1] + s.origin[1], f.target[2] + s.origin[2]];
        s.view.cam.persp = true;
        s.view.fit_pending = false;
        let out = format!(
            "{}{}",
            format_station(f.sta, &units, None),
            if f.z.is_finite() { format!("  EL {}", fixed(f.z, 2)) } else { String::new() }
        );
        if let Some(dd) = self.drive.as_mut() {
            dd.sta = f.sta;
            dd.out = out;
            dd.last_z = last;
        }
    }

    /// `civil.drive=…`: on | off | al=<id> | sta=<station> | play | pause | step=+1|-1 | speed=<i>.
    pub fn drive_command(&mut self, spec: &str, dark: bool) -> Result<(), String> {
        let (k, v) = spec.split_once('=').map_or((spec.trim(), ""), |(a, b)| (a.trim(), b.trim()));
        match k {
            "on" | "" if self.drive.is_none() => {
                let list = self.drive_alignments();
                let Some(first) = list.first() else {
                    self.message = t("dv.none", &[]);
                    return Ok(());
                };
                self.drive_begin(&string_of(first.get("id")), dark);
            }
            "on" | "" => {}
            "off" => self.drive_end(),
            "al" => {
                if !self.drive_alignments().iter().any(|a| a.get("id").and_then(Value::as_str) == Some(v)) {
                    return Err(format!("civil.drive: no alignment {v} to drive"));
                }
                self.drive_begin(v, dark);
            }
            _ if self.drive.is_none() => return Err("civil.drive: not driving (civil.drive=on)".into()),
            "sta" => {
                let x = cc_civil::js::str_to_number(v);
                if !x.is_finite() {
                    return Err("civil.drive: sta = a number".into());
                }
                if let Some(d) = self.drive.as_mut() {
                    d.playing = false;
                    d.sta = x;
                }
                self.drive_place();
            }
            "play" | "pause" => self.drive_playing(k == "play"),
            "step" => {
                let dir = if v.starts_with('-') { -1.0 } else { 1.0 };
                let step = if self.project.units() == "m" { 5.0 } else { 10.0 };
                if let Some(d) = self.drive.as_mut() {
                    d.playing = false;
                    d.sta += dir * step;
                }
                self.drive_place();
            }
            "clock" => {
                if let Some(d) = self.drive.as_mut() {
                    d.manual = v == "manual";
                }
            }
            "speed" => {
                let i: usize = v.parse().map_err(|_| "civil.drive: speed = 0, 1, 2 or 3")?;
                if i >= 4 {
                    return Err("civil.drive: speed = 0, 1, 2 or 3".into());
                }
                if let Some(d) = self.drive.as_mut() {
                    d.speed = i;
                }
            }
            "tick" => {
                // headless time: v seconds of playing (as requestAnimationFrame's steps, at most 0.1 s each)
                let secs = cc_civil::js::str_to_number(v);
                if !(secs.is_finite() && secs > 0.0) {
                    return Err("civil.drive: tick = seconds".into());
                }
                let mut left = secs.min(3600.0);
                while left > 0.0 && self.drive.as_ref().is_some_and(|d| d.playing) {
                    let dt = left.min(0.1);
                    left -= dt;
                    self.drive_advance(dt);
                }
            }
            o => return Err(format!("civil.drive: on, off, al=, sta=, play, pause, step=, speed= or tick= (not {o})")),
        }
        Ok(())
    }

    fn drive_begin(&mut self, id: &str, dark: bool) {
        self.show3d = true;
        // drive-ui.js begin: rebuild() first (the 3D data of this version of the project)
        if self.site3.as_ref().is_none_or(|s| s.version != self.project.version || (s.exag - self.exag).abs() > 1e-9) {
            let keep = self.site3.as_ref().map(|s| s.view.cam);
            let mut s = crate::civil3d::build(&mut self.project, dark, self.exag, (1 << 41) + 7);
            if let Some(c) = keep {
                s.view.cam = c;
                s.view.fit_pending = false;
            }
            self.site3 = Some(s);
        }
        let Some(a) = arr(self.project.state.get("alignments")).iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id)).cloned()
        else {
            return;
        };
        let al = self.project.alignment(&a);
        let saved = match &self.drive {
            Some(d) => d.saved,
            None => self.site3.as_ref().map(|s| s.view.cam),
        };
        let speed = self.drive.as_ref().map_or(1, |d| d.speed);
        if let Some(s) = self.site3.as_mut() {
            s.view.cam.fov = 50.0;
        }
        self.drive = Some(Drive {
            al: id.to_string(),
            sta: al.start,
            playing: false,
            speed,
            saved,
            out: String::new(),
            last: None,
            last_z: None,
            manual: self.drive.as_ref().is_some_and(|d| d.manual),
        });
        self.drive_place();
        let prof_ok = self.project.design_profile(&a).ok();
        self.message = tr!(if prof_ok { "dv.on.prof" } else { "dv.on.eg" }, name = string_of(a.get("name")));
    }

    fn drive_end(&mut self) {
        let Some(d) = self.drive.take() else { return };
        if let (Some(c), Some(s)) = (d.saved, self.site3.as_mut()) {
            s.view.cam = c;
        }
        self.message = t("dv.off", &[]);
    }

    fn drive_playing(&mut self, on: bool) {
        let end = self.drive_end_sta();
        let start = self.drive_start_sta();
        if let Some(d) = self.drive.as_mut() {
            if on && d.sta >= end - 1e-6 {
                d.sta = start;
            }
            d.playing = on;
            d.last = None;
        }
    }

    fn drive_al(&mut self) -> Option<std::sync::Arc<cc_civil::alignment::Alignment>> {
        let id = self.drive.as_ref()?.al.clone();
        let a =
            arr(self.project.state.get("alignments")).iter().find(|a| a.get("id").and_then(Value::as_str) == Some(id.as_str())).cloned()?;
        Some(self.project.alignment(&a))
    }
    fn drive_end_sta(&mut self) -> f64 {
        self.drive_al().map_or(0.0, |a| a.end)
    }
    fn drive_start_sta(&mut self) -> f64 {
        self.drive_al().map_or(0.0, |a| a.start)
    }

    /// One step of playing (drive-ui.js `tick`): the speed × dt along; at the end it stops there.
    pub fn drive_advance(&mut self, dt: f64) {
        let units = self.project.units().to_string();
        let end = self.drive_end_sta();
        let Some(d) = self.drive.as_mut() else { return };
        if !d.playing {
            return;
        }
        let per = cc_civil::drive::drive_speeds(&units).get(d.speed).map_or(0.0, |s| s.1);
        d.sta += per * dt.min(0.1);
        let stop = d.sta >= end;
        if stop {
            d.sta = end;
        }
        self.drive_place();
        if stop && let Some(d) = self.drive.as_mut() {
            d.playing = false;
        }
    }

    /// `civil.drive.info`: the station readout and whether it plays.
    pub fn drive_info(&self) -> String {
        match &self.drive {
            Some(d) => format!("{} · {}", d.out, t0(if d.playing { "dv.pause" } else { "dv.play" })),
            None => String::new(),
        }
    }

    /// `civil.sun=on=…,shadows=…,date=…,time=…,utc=…,lat=…,lon=…`, then drive-ui.js `applySun()`.
    pub fn sun_command(&mut self, spec: &str, now_ms: f64, zone: cc_clock::Zone) -> Result<(), String> {
        // "now=<ms>": a fixed clock for the defaults (tests)
        let now_ms = spec
            .split(',')
            .find_map(|kv| kv.trim().strip_prefix("now=").map(cc_civil::js::str_to_number))
            .filter(|x| x.is_finite())
            .unwrap_or(now_ms);
        let mut s = self.sun.clone().unwrap_or_else(|| SunSet::defaults(now_ms, zone));
        for kv in spec.split(',').map(str::trim).filter(|x| !x.is_empty()) {
            let (k, v) = kv.split_once('=').map_or((kv, ""), |(a, b)| (a.trim(), b.trim()));
            let on_off = || match v {
                "on" | "true" => Ok(true),
                "off" | "false" => Ok(false),
                o => Err(format!("civil.sun: {k} = on or off (not {o})")),
            };
            match k {
                "on" => s.on = on_off()?,
                "shadows" => s.shadows = on_off()?,
                "date" => s.date = v.chars().take(10).collect(),
                "time" => s.time = v.chars().take(5).collect(),
                "utc" => {
                    let x = cc_civil::js::str_to_number(v);
                    if !(x.is_finite() && (-12.0..=14.0).contains(&x) && (x * 2.0).fract() == 0.0) {
                        return Err("civil.sun: utc = −12 … +14 in half hours".into());
                    }
                    s.utc = x;
                }
                "lat" => s.lat = v.chars().take(19).collect(),
                "lon" => s.lon = v.chars().take(19).collect(),
                "now" => {}
                o => return Err(format!("civil.sun: unknown field {o}")),
            }
        }
        self.sun = Some(s);
        self.apply_sun();
        Ok(())
    }

    /// `civil.sun.info`: the menu's fields and its readout.
    pub fn sun_info(&self, now_ms: f64, zone: cc_clock::Zone) -> String {
        let s = self.sun.clone().unwrap_or_else(|| SunSet::defaults(now_ms, zone));
        format!(
            "on={} shadows={} date={} time={} utc={} lat={} lon={} · {}",
            s.on,
            s.shadows,
            s.date,
            s.time,
            cc_civil::js::num_to_string(s.utc),
            s.lat,
            s.lon,
            self.sun_out
        )
    }

    /// drive-ui.js `applySun()`: the light of the sun's position (or the usual light), and the readout.
    pub fn apply_sun(&mut self) {
        let Some(s) = self.sun.clone() else { return };
        let set = |cv: &mut CivilView, sun: Option<cc_render::mesh3d::Sun3>| {
            if let Some(x) = cv.site3.as_mut() {
                x.view.style.sun = sun;
            }
        };
        if !s.on {
            set(self, None);
            self.sun_out.clear();
            return;
        }
        let (la, lo) = (cc_civil::js::str_to_number(&s.lat), cc_civil::js::str_to_number(&s.lon));
        let when = s.when();
        let bad =
            la.is_nan() || la.abs() > 90.0 || lo.is_nan() || lo.abs() > 180.0 || s.lat.is_empty() || s.lon.is_empty() || when.is_none();
        let Some(when) = when.filter(|_| !bad) else {
            set(self, None);
            let crs = self.project.state.get("crs").is_some_and(|c| c.is_object());
            self.sun_out = t(if crs { "sun.need" } else { "sun.need.crs" }, &[]);
            return;
        };
        let pos = cc_civil::drive::sun_position(when, la, lo);
        let (az, alt) = (pos.first().copied().unwrap_or(0.0), pos.get(1).copied().unwrap_or(0.0));
        let v = cc_civil::drive::sun_vector(az, alt);
        let ex = self.site3.as_ref().map_or(self.exag, |x| x.exag);
        let d = [v[0], v[1], v[2] * ex];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-12);
        // shadows off: the sun still lights the faces (three.js's light without castShadow); the renderer's sun casts
        // shadows, so off keeps the usual light
        let sun = s.shadows.then_some(cc_render::mesh3d::Sun3 { dir: [d[0] / l, d[1] / l, d[2] / l], up: alt > 0.0 });
        set(self, sun);
        self.sun_out = if alt > 0.0 { tr!("sun.at", az = fixed(az, 1), alt = fixed(alt, 1)) } else { tr!("sun.below", az = fixed(az, 1)) };
    }
}

/// The Drive panel over the 3D view and the Drive / Sun buttons (drawn by civilview's 3D bar).
pub fn bar(cv: &mut CivilView, ui: &mut egui::Ui, acts: &mut Vec<(&'static str, String)>) {
    if ui.selectable_label(cv.drive.is_some(), t0("dv.btn")).on_hover_text(t0("dv.title")).clicked() {
        acts.push(("civil.drive", if cv.drive.is_some() { "off".into() } else { "on".into() }));
    }
    let open = cv.sun.as_ref().is_some_and(|s| s.on);
    ui.menu_button(if open { format!("☀ {}", t0("sun.btn")) } else { t0("sun.btn") }, |ui| {
        let s = cv.sun.clone().unwrap_or_else(|| SunSet::defaults(cc_clock::now_ms(), cc_clock::Zone::System));
        let mut on = s.on;
        if ui.checkbox(&mut on, t0("sun.on")).changed() {
            acts.push(("civil.sun", format!("on={}", if on { "on" } else { "off" })));
        }
        let mut sh = s.shadows;
        if ui.checkbox(&mut sh, t0("sun.shadows")).changed() {
            acts.push(("civil.sun", format!("shadows={}", if sh { "on" } else { "off" })));
        }
        sun_field(cv, ui, acts, "sun.date", &s.date, "date", 96.0);
        sun_field(cv, ui, acts, "sun.time", &s.time, "time", 56.0);
        egui::ComboBox::new("cv_sun_utc", t0("sun.utc")).selected_text(utc_label(s.utc)).show_ui(ui, |ui| {
            for i in 0..53 {
                let h = -12.0 + f64::from(i) / 2.0;
                if ui.selectable_label(h == s.utc, utc_label(h)).clicked() {
                    acts.push(("civil.sun", format!("utc={}", cc_civil::js::num_to_string(h))));
                }
            }
        });
        sun_field(cv, ui, acts, "sun.lat", &s.lat, "lat", 90.0);
        sun_field(cv, ui, acts, "sun.lon", &s.lon, "lon", 90.0);
        if !cv.sun_out.is_empty() {
            ui.label(RichText::new(&cv.sun_out).small());
        }
        ui.label(RichText::new(t0("sun.note")).small().weak());
    });
}

/// One typed field of the Sun menu: committed when it loses focus with a change.
#[allow(clippy::too_many_arguments)]
fn sun_field(cv: &mut CivilView, ui: &mut egui::Ui, acts: &mut Vec<(&'static str, String)>, key: &str, cur: &str, k: &str, w: f32) {
    ui.horizontal(|ui| {
        let l = ui.label(t0(key));
        let mut text = cv.sun_edit.get(k).cloned().unwrap_or_else(|| cur.to_string());
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(w)).labelled_by(l.id);
        if r.changed() {
            cv.sun_edit.insert(k.to_string(), text.clone());
        }
        if r.lost_focus()
            && let Some(v) = cv.sun_edit.remove(k)
            && v != cur
        {
            acts.push(("civil.sun", format!("{k}={}", v.replace(',', ""))));
        }
    });
}

fn utc_label(h: f64) -> String {
    let a = h.abs();
    format!("UTC{}{:02}:{}", if h < 0.0 { '−' } else { '+' }, a.floor() as i64, if a.fract() != 0.0 { "30" } else { "00" })
}

/// The Drive panel (top left of the 3D view) while driving.
pub fn panel(cv: &mut CivilView, ui: &mut egui::Ui, rect: egui::Rect, tk: &Tokens, acts: &mut Vec<(&'static str, String)>) {
    let Some(d) = cv.drive.clone() else { return };
    let units = cv.project.units().to_string();
    let als: Vec<(String, String)> = cv.drive_alignments().iter().map(|a| (string_of(a.get("id")), string_of(a.get("name")))).collect();
    let (start, end) = (cv.drive_start_sta(), cv.drive_end_sta());
    egui::Area::new(egui::Id::unique("cv_drive_panel")).fixed_pos(rect.min + egui::vec2(12.0, 12.0)).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(tk.panel).stroke(egui::Stroke::new(1.5, tk.text)).inner_margin(6).corner_radius(3).show(ui, |ui| {
            ui.horizontal(|ui| {
                let cur = als.iter().find(|(id, _)| *id == d.al).map_or(String::new(), |x| x.1.clone());
                egui::ComboBox::new("cv_dv_al", "").selected_text(cur).show_ui(ui, |ui| {
                    for (id, name) in &als {
                        if ui.selectable_label(*id == d.al, name).clicked() {
                            acts.push(("civil.drive", format!("al={id}")));
                        }
                    }
                });
                if ui.selectable_label(d.playing, t0(if d.playing { "dv.pause" } else { "dv.play" })).clicked() {
                    acts.push(("civil.drive", if d.playing { "pause".into() } else { "play".into() }));
                }
                let mut sta = d.sta;
                if ui
                    .add(egui::Slider::new(&mut sta, start..=end.max(start + 1e-9)).show_value(false))
                    .on_hover_text(t0("dv.sta"))
                    .changed()
                {
                    acts.push(("civil.drive", format!("sta={sta}")));
                }
                ui.label(RichText::new(&d.out).monospace());
                let speeds = cc_civil::drive::drive_speeds(&units);
                egui::ComboBox::new("cv_dv_speed", "").selected_text(speeds.get(d.speed).map_or(String::new(), |s| s.0.clone())).show_ui(
                    ui,
                    |ui| {
                        for (i, (label, _)) in speeds.iter().enumerate() {
                            if ui.selectable_label(i == d.speed, label).clicked() {
                                acts.push(("civil.drive", format!("speed={i}")));
                            }
                        }
                    },
                );
                if ui.button(t0("dv.stop")).clicked() {
                    acts.push(("civil.drive", "off".into()));
                }
            });
        });
    });
    if d.playing && !d.manual {
        let dt = f64::from(ui.input(|i| i.stable_dt)).min(0.1);
        cv.drive_advance(dt);
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_defaults_follow_the_zone_and_dates_are_checked() {
        let s = SunSet::defaults(1_760_700_000_000.0, cc_clock::Zone::Fixed(-360));
        assert_eq!((s.date.as_str(), s.utc), ("2025-10-17", -6.0));
        assert_eq!(SunSet::defaults(0.0, cc_clock::Zone::Fixed(330)).utc, 5.5, "half hours are kept");
        let mut x = s.clone();
        x.date = "2026-02-30".into();
        assert!(x.when().is_none());
        x.date = "2026-06-21".into();
        x.time = "07:30".into();
        x.utc = -6.5;
        // 2026-06-21 07:30 at UTC−6:30 = 14:00 UTC
        assert_eq!(x.when(), Some(1_782_050_400_000.0));
    }

    #[test]
    fn driving_moves_along_and_stops_at_the_end() {
        let mut cv = CivilView::demo().unwrap();
        cv.drive_command("on", true).unwrap();
        assert!(cv.drive.is_some());
        cv.drive_command("speed=3", true).unwrap();
        cv.drive_command("play", true).unwrap();
        cv.drive_command("tick=600", true).unwrap();
        let d = cv.drive.clone().unwrap();
        assert!(!d.playing && d.out.contains("EL"), "{}", d.out);
        cv.drive_command("off", true).unwrap();
        assert!(cv.drive.is_none());
        assert!(cv.drive_command("step=1", true).is_err());
        cv.sun_command("on=on,lat=39.74,lon=-104.99,date=2026-06-21,time=09:00,utc=-6", 0.0, cc_clock::Zone::Utc).unwrap();
        assert!(cv.sun_out.contains("azimuth"), "{}", cv.sun_out);
        assert!(cv.sun_command("utc=99", 0.0, cc_clock::Zone::Utc).is_err());
    }
}
