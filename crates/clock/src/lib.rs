//! CadCraft L0: the host's clock edge — the system time zone's UTC offset at an instant.
//!
//! The 0.5 JS writes some dates in the user's local time (zip DOS dates, LandXML `date` / `time`, the .dbf header,
//! KISS headers, list dates) and others in UTC (`toISOString`, PDF dates). The engine and the writers read no clock
//! and no zone: the host (the app, the CLI, the MCP server) asks this crate for the offset of the instant it already
//! passes and hands both to the writer. The offset is per instant (daylight saving), as JS's
//! `-new Date(ms).getTimezoneOffset()`.
//!
//! Native: the `TZ` variable, else `/etc/localtime` (Unix) or the Windows zone, through `jiff`. Web: the browser's
//! `Date.prototype.getTimezoneOffset`. Tests fix the zone per thread with [`with_offset`].

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use std::cell::Cell;

/// The widest offset any zone has used (±26 h covers every real zone; tzdb's span is −12:00 … +14:00).
const MAX_OFFSET_MIN: i32 = 26 * 60;

thread_local! {
    static FIXED: Cell<Option<i32>> = const { Cell::new(None) };
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static NAMED: std::cell::RefCell<Option<jiff::tz::TimeZone>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` with this thread's system zone replaced by the IANA zone `name` (daylight saving included): tests of
/// the per-instant offsets (DST edges, half-hour zones) without touching the process's `TZ`. Native only.
#[cfg(not(target_arch = "wasm32"))]
pub fn with_zone_name<R>(name: &str, f: impl FnOnce() -> R) -> Result<R, String> {
    let tz = jiff::tz::TimeZone::get(name).map_err(|e| format!("time zone {name}: {e}"))?;
    let prev = NAMED.with(|c| c.replace(Some(tz)));
    let out = f();
    NAMED.with(|c| *c.borrow_mut() = prev);
    Ok(out)
}

/// Runs `f` with this thread's zone fixed at `offset_min` (minutes east of UTC): tests and scripted runs that must
/// not depend on the machine's zone.
pub fn with_offset<R>(offset_min: i32, f: impl FnOnce() -> R) -> R {
    let prev = FIXED.with(|c| c.replace(Some(offset_min.clamp(-MAX_OFFSET_MIN, MAX_OFFSET_MIN))));
    let out = f();
    FIXED.with(|c| c.set(prev));
    out
}

/// The time now in ms since 1970 (UTC).
pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0).floor()
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
}

/// The local zone's offset from UTC at the instant `ms` (ms since 1970), in minutes east of UTC: JS's
/// `-new Date(ms).getTimezoneOffset()`. 0 for a time JS cannot represent, or when the zone cannot be read.
pub fn utc_offset_min(ms: f64) -> i32 {
    if let Some(m) = FIXED.with(Cell::get) {
        return m;
    }
    if !ms.is_finite() || ms.abs() > 8.64e15 {
        return 0;
    }
    system_offset_min(ms)
}

/// `ms` moved to the local wall clock: civil fields read from it as UTC are JS's `getHours()`, `getDate()` … of `ms`.
pub fn local_ms(ms: f64) -> f64 {
    Zone::System.local_ms(ms)
}

/// The zone a writer reads local-time fields in, passed down by the host with the clock value.
///
/// Writers below the UI default to `Utc` (deterministic); the app, the CLI and the MCP server pass `System`, or
/// `Fixed(m)` when a script, a command (`app.tz`) or a test fixes the zone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Zone {
    /// The machine's zone (or the browser's), per instant, through [`utc_offset_min`].
    System,
    /// A fixed offset in minutes east of UTC (JS's `-getTimezoneOffset()`).
    Fixed(i32),
    /// UTC.
    #[default]
    Utc,
}

impl Zone {
    /// A zone from a script's `tz` value (minutes east of UTC); `None` or a non-finite value is UTC.
    pub fn from_minutes(m: Option<f64>) -> Zone {
        match m {
            Some(v) if v.is_finite() => {
                // clamped to ±26 h before the cast
                #[allow(clippy::cast_possible_truncation)]
                let m = v.round().clamp(f64::from(-MAX_OFFSET_MIN), f64::from(MAX_OFFSET_MIN)) as i32;
                Zone::Fixed(m)
            }
            _ => Zone::Utc,
        }
    }
    /// Minutes east of UTC at the instant `ms`.
    pub fn offset_min(self, ms: f64) -> i32 {
        match self {
            Zone::System => utc_offset_min(ms),
            Zone::Fixed(m) => m.clamp(-MAX_OFFSET_MIN, MAX_OFFSET_MIN),
            Zone::Utc => 0,
        }
    }
    /// `ms` on the local wall clock (UTC civil fields of the result = JS's local `get…()` of `ms`); NaN stays NaN.
    pub fn local_ms(self, ms: f64) -> f64 {
        if ms.is_finite() { ms + f64::from(self.offset_min(ms)) * 60_000.0 } else { ms }
    }
    /// The instant of a local wall-clock time (JS's `new Date(y, m, d, h, …)` and its parsing of an ISO date-time
    /// without an offset). Where the wall clock is ambiguous V8 takes the offset in force BEFORE the change: a time in
    /// a spring-forward gap (02:30 on 2026-03-08 in Denver) is read with standard time (→ 09:30Z, shown as 03:30
    /// MDT), a time that occurs twice in autumn (01:30 on 2026-11-01) is the first one (daylight time, 07:30Z).
    pub fn utc_of_local(self, local: f64) -> f64 {
        if !local.is_finite() {
            return local;
        }
        let at = |off: i32| local - f64::from(off) * 60_000.0;
        // the instants a wall-clock time can be lie within [local − 14 h, local + 12 h] (offsets −12:00 … +14:00);
        // the offsets at both ends are the one before and the one after any change near it
        let before = self.offset_min(local - 14.0 * 3_600_000.0);
        let after = self.offset_min(local + 12.0 * 3_600_000.0);
        if self.offset_min(at(before)) == before {
            at(before)
        } else if self.offset_min(at(after)) == after {
            at(after)
        } else {
            // a gap: the wall-clock time never happens; V8 reads it with the offset before the change
            at(before)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn system_offset_min(ms: f64) -> i32 {
    use std::sync::OnceLock;
    static ZONE: OnceLock<jiff::tz::TimeZone> = OnceLock::new();
    // |ms| ≤ 8.64e15 (checked by the caller), so the cast is exact
    #[allow(clippy::cast_possible_truncation)]
    let whole = ms.floor() as i64;
    let Ok(ts) = jiff::Timestamp::from_millisecond(whole) else { return 0 };
    // whole minutes toward zero, as V8's getTimezoneOffset() (local mean time before 1883 in Denver: −6:59:56 → −419)
    let of = |zone: &jiff::tz::TimeZone| (zone.to_offset(ts).seconds() / 60).clamp(-MAX_OFFSET_MIN, MAX_OFFSET_MIN);
    if let Some(m) = NAMED.with(|c| c.borrow().as_ref().map(of)) {
        return m;
    }
    of(ZONE.get_or_init(jiff::tz::TimeZone::system))
}

#[cfg(target_arch = "wasm32")]
fn system_offset_min(ms: f64) -> i32 {
    let d = js_sys::Date::new_0();
    d.set_time(ms);
    let west = d.get_timezone_offset();
    if !west.is_finite() {
        return 0;
    }
    // a finite offset in minutes, clamped to ±26 h before the cast
    #[allow(clippy::cast_possible_truncation)]
    let east = (-west).round().clamp(f64::from(-MAX_OFFSET_MIN), f64::from(MAX_OFFSET_MIN)) as i32;
    east
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_zone_is_per_thread_and_restored() {
        let t = 1_791_806_400_000.0;
        let before = utc_offset_min(t);
        assert_eq!(with_offset(330, || utc_offset_min(t)), 330);
        assert_eq!(with_offset(-360, || local_ms(t)), t - 360.0 * 60_000.0);
        assert_eq!(with_offset(825, || with_offset(-360, || utc_offset_min(t))), -360);
        assert_eq!(utc_offset_min(t), before);
    }

    #[test]
    fn zones_move_the_wall_clock() {
        let t = 1_791_806_400_000.0; // 2026-10-12T12:00:00Z
        assert_eq!(Zone::Fixed(330).local_ms(t), t + 330.0 * 60_000.0);
        assert_eq!(Zone::Fixed(-360).local_ms(t), t - 360.0 * 60_000.0);
        assert_eq!(Zone::Utc.local_ms(t), t);
        assert_eq!(Zone::Fixed(825).utc_of_local(Zone::Fixed(825).local_ms(t)), t);
        assert!(Zone::Fixed(60).local_ms(f64::NAN).is_nan());
        assert_eq!(Zone::from_minutes(Some(-359.6)), Zone::Fixed(-360));
        assert_eq!(Zone::from_minutes(Some(1e12)), Zone::Fixed(MAX_OFFSET_MIN));
        assert_eq!(Zone::from_minutes(None), Zone::Utc);
        assert_eq!(with_offset(345, || Zone::System.offset_min(t)), 345);
    }

    #[test]
    fn hostile_times_give_utc() {
        assert_eq!(with_offset(0, || utc_offset_min(f64::NAN)), 0);
        assert_eq!(utc_offset_min(f64::INFINITY), 0);
        assert_eq!(utc_offset_min(9e15), 0);
    }

    #[test]
    fn the_system_zone_reads_without_failing() {
        // whatever the machine's zone, the offset is within the span real zones use
        let m = utc_offset_min(now_ms());
        assert!((-MAX_OFFSET_MIN..=MAX_OFFSET_MIN).contains(&m));
    }
    /// ms of a UTC date-time (test helper).
    #[allow(clippy::arithmetic_side_effects, clippy::cast_precision_loss)]
    fn utc(y: i64, mo: i64, d: i64, h: i64, mi: i64) -> f64 {
        // days from civil (H. Hinnant)
        let (y, mo) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * mo + 2) / 5 + d - 1;
        let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
        ((days * 86_400 + h * 3600 + mi * 60) * 1000) as f64
    }

    /// Node 22 (V8) under TZ=<zone>: `new Date(y, m, d, h, mi).toISOString()` and `-getTimezoneOffset()`.
    #[test]
    fn named_zones_match_v8_at_dst_edges() {
        let case = |zone: &str, f: &dyn Fn()| with_zone_name(zone, f).unwrap();
        case("America/Denver", &|| {
            // spring forward 2026-03-08 09:00Z: 02:30 never happens → read with MST (V8: 09:30Z)
            assert_eq!(Zone::System.utc_of_local(utc(2026, 3, 8, 2, 30)), utc(2026, 3, 8, 9, 30));
            assert_eq!(Zone::System.utc_of_local(utc(2026, 3, 8, 1, 59)), utc(2026, 3, 8, 8, 59));
            assert_eq!(Zone::System.utc_of_local(utc(2026, 3, 8, 3, 0)), utc(2026, 3, 8, 9, 0));
            // fall back 2026-11-01 08:00Z: 01:30 happens twice → the first (MDT, 07:30Z)
            assert_eq!(Zone::System.utc_of_local(utc(2026, 11, 1, 1, 30)), utc(2026, 11, 1, 7, 30));
            assert_eq!(utc_offset_min(utc(2026, 11, 1, 7, 59)), -360);
            assert_eq!(utc_offset_min(utc(2026, 11, 1, 8, 0)), -420);
            // the local wall clock either side of the change
            assert_eq!(local_ms(utc(2026, 3, 8, 8, 59)), utc(2026, 3, 8, 1, 59));
            assert_eq!(local_ms(utc(2026, 3, 8, 9, 0)), utc(2026, 3, 8, 3, 0));
            // local mean time before 1883 (−6:59:56): V8's getTimezoneOffset() is 419 (whole minutes)
            assert_eq!(utc_offset_min(utc(1880, 1, 1, 12, 0)), -419);
            assert_eq!(utc_offset_min(utc(1975, 6, 1, 18, 0)), -360);
        });
        case("Pacific/Auckland", &|| {
            // southern hemisphere: NZDT (+13) from 2026-09-27 02:00 local, NZST (+12) from 2026-04-05 03:00 local
            assert_eq!(Zone::System.utc_of_local(utc(2026, 9, 27, 2, 30)), utc(2026, 9, 26, 14, 30));
            assert_eq!(Zone::System.utc_of_local(utc(2026, 4, 5, 2, 30)), utc(2026, 4, 4, 13, 30));
            assert_eq!(utc_offset_min(utc(2026, 1, 15, 0, 0)), 780);
            assert_eq!(utc_offset_min(utc(2026, 7, 15, 0, 0)), 720);
            // a UTC date that is already tomorrow on the local clock
            assert_eq!(local_ms(utc(2026, 12, 31, 11, 30)), utc(2027, 1, 1, 0, 30));
        });
        case("Asia/Kolkata", &|| {
            assert_eq!(utc_offset_min(utc(2026, 10, 12, 13, 2)), 330);
            assert_eq!(Zone::System.utc_of_local(utc(2026, 10, 12, 18, 32)), utc(2026, 10, 12, 13, 2));
        });
        case("Pacific/Chatham", &|| {
            // +13:45 in October, +12:45 in July
            assert_eq!(utc_offset_min(utc(2026, 10, 12, 13, 2)), 825);
            assert_eq!(utc_offset_min(utc(2026, 7, 1, 0, 0)), 765);
        });
        // a fixed offset wins over a named zone; an unknown zone is an error, not a panic
        assert_eq!(with_zone_name("America/Denver", || with_offset(60, || utc_offset_min(0.0))).unwrap(), 60);
        assert!(with_zone_name("Nowhere/Atlantis", || ()).is_err());
    }
}
