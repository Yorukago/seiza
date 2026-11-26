//! Where Voyager 1 is, worked out on this computer from one state vector.
//!
//! The vector (heliocentric, ecliptic J2000) is JPL Horizons data for 2026-11-21 00:00 TDB. Voyager
//! has escaped the Sun, so its path is a straight line to well within what a screen can show.
//! That one is built in. `refresh_if_stale` asks Horizons for a newer one at most once a day and keeps it in
//! ~/.cache/seiza/voyager, which is used from then on; with no network the built-in one still works.
//! shortcut: linear motion, error under ~0.1 AU for ten years either side of the vector's date, so a refresh
//! is about keeping that date near, not about the screen showing a difference.

use std::{path::PathBuf, process::Command, sync::Mutex, time::Duration};

const EPOCH_JD: f64 = 2_461_365.5;
const POS_KM: [f64; 3] = [-4.820_639_498_457_286e9, -2.052_864_545_855_836e10, 1.485_279_842_180_252e10];
const VEL_KMS: [f64; 3] = [-2.069_393_046_020_428, -13.611_368_248_445_48, 9.831_456_689_918_61];

const AU_KM: f64 = 149_597_870.7;
const LIGHT_KMS: f64 = 299_792.458;
const OBLIQUITY_DEG: f64 = 23.439_291_1;
const J2000_JD: f64 = 2_451_545.0;
const UNIX_EPOCH_JD: f64 = 2_440_587.5;

#[derive(Clone, Copy, Debug)]
pub struct Voyager {
    pub au: f64,         // distance from the Sun
    pub speed_kms: f64,  // speed relative to the Sun
    pub earth_au: f64,   // distance from Earth
    pub light_secs: f64, // one-way light time to Earth
    pub ra_hours: f64,   // direction in the sky, equatorial J2000
    pub dec_deg: f64,
}

/// One measured state of the probe: Julian date, then position (km) and velocity (km/s), heliocentric ecliptic J2000.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector {
    jd: f64,
    pos: [f64; 3],
    vel: [f64; 3],
}

const BUILT_IN: Vector = Vector { jd: EPOCH_JD, pos: POS_KM, vel: VEL_KMS };

/// The vector in use: the cached one if there is one, read once, else the built-in one.
static LIVE: Mutex<Option<Vector>> = Mutex::new(None);

fn cache_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"));
    base.join("seiza").join("voyager")
}

/// Something that is not Voyager 1 (an error page, another object) must never get in.
fn sane(v: &Vector) -> bool {
    let (au, kms) = (norm(v.pos) / AU_KM, norm(v.vel));
    v.jd > 2_460_000.0 && (100.0..400.0).contains(&au) && (10.0..30.0).contains(&kms)
}

fn read_cache() -> Option<Vector> {
    let n: Vec<f64> = std::fs::read_to_string(cache_path()).ok()?.split_whitespace().filter_map(|w| w.parse().ok()).collect();
    let v = Vector { jd: *n.first()?, pos: [*n.get(1)?, *n.get(2)?, *n.get(3)?], vel: [*n.get(4)?, *n.get(5)?, *n.get(6)?] };
    sane(&v).then_some(v)
}

fn current() -> Vector {
    *LIVE.lock().unwrap().get_or_insert_with(|| read_cache().unwrap_or(BUILT_IN))
}

/// The vector out of a Horizons answer (VEC_TABLE 2, km and km/s): the three lines between $$SOE and $$EOE.
pub fn parse_horizons(text: &str) -> Option<Vector> {
    let mut rows = text.split("$$SOE").nth(1)?.split("$$EOE").next()?.lines().map(str::trim).filter(|l| !l.is_empty());
    let jd = rows.next()?.split_whitespace().next()?.parse().ok()?;
    // " X =-4.8E+09 Y =-2.0E+10 Z = 1.4E+10": the first word after each '='
    let three = |line: &str| -> Option<[f64; 3]> {
        let w: Vec<f64> = line.split('=').skip(1).filter_map(|part| part.split_whitespace().next()?.parse().ok()).collect();
        Some([*w.first()?, *w.get(1)?, *w.get(2)?])
    };
    let v = Vector { jd, pos: three(rows.next()?)?, vel: three(rows.next()?)? };
    sane(&v).then_some(v)
}

/// Ask NASA's Horizons for Voyager 1's state now, unless the cache is under a day old. True when a new one is in use.
pub fn refresh_if_stale() -> bool {
    let path = cache_path();
    let fresh = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(20 * 3600));
    if fresh {
        return false;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let jd = format!("'{:.1}'", (now / 86_400.0 + UNIX_EPOCH_JD).floor() + 0.5); // midnight UTC of today, a time Horizons has a row for
    let query = [("format", "text"), ("COMMAND", "'-31'"), ("OBJ_DATA", "'NO'"), ("MAKE_EPHEM", "'YES'"), ("EPHEM_TYPE", "'VECTORS'"), ("CENTER", "'500@10'"), ("TLIST", &jd), ("VEC_TABLE", "'2'"), ("OUT_UNITS", "'KM-S'"), ("CSV_FORMAT", "'NO'"), ("REF_PLANE", "'ECLIPTIC'"), ("REF_SYSTEM", "'ICRF'")];
    let mut curl = Command::new("curl");
    curl.args(["-sf", "-m", "20", "-G", "https://ssd.jpl.nasa.gov/api/horizons.api"]);
    for (k, v) in query {
        curl.arg("--data-urlencode").arg(format!("{k}={v}"));
    }
    let Some(v) = curl.output().ok().filter(|o| o.status.success()).and_then(|o| parse_horizons(&String::from_utf8_lossy(&o.stdout))) else { return false };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, format!("{} {} {} {} {} {} {}\n", v.jd, v.pos[0], v.pos[1], v.pos[2], v.vel[0], v.vel[1], v.vel[2]));
    *LIVE.lock().unwrap() = Some(v);
    true
}

/// Earth's heliocentric ecliptic position in km, on a circular 1 AU orbit.
/// shortcut: real orbit is up to 1.7% off in radius and a degree or two in angle, so Earth distance is
/// good to about 0.04 AU; fine at 170 AU, use a proper ephemeris if it ever matters.
fn earth_km(jd: f64) -> [f64; 3] {
    let sun_lon = (280.46 + 0.985_647_4 * (jd - J2000_JD)).rem_euclid(360.0);
    let lon = (sun_lon + 180.0).to_radians();
    [AU_KM * lon.cos(), AU_KM * lon.sin(), 0.0]
}

fn norm(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// A light time as hours and minutes, or minutes and seconds under an hour.
pub fn light_text(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    match total {
        0..=3599 => format!("{} min {:02} s", total / 60, total % 60),
        _ => format!("{} h {:02} min", total / 3600, total % 3600 / 60),
    }
}

/// Voyager 1 at a Unix time in seconds (UTC; the 69 s to TDB is far below what shows).
pub fn voyager_at(unix_secs: f64) -> Voyager {
    voyager_with(current(), unix_secs)
}

fn voyager_with(v0: Vector, unix_secs: f64) -> Voyager {
    let jd = unix_secs / 86_400.0 + UNIX_EPOCH_JD;
    let dt = (jd - v0.jd) * 86_400.0;
    let p = [0, 1, 2].map(|i| v0.pos[i] + v0.vel[i] * dt);
    let e = earth_km(jd);
    let earth_km_away = norm([p[0] - e[0], p[1] - e[1], p[2] - e[2]]);
    // ecliptic -> equatorial: rotate about x by the obliquity. Sun-centred direction, parallax <= 0.33 deg.
    let (s, c) = OBLIQUITY_DEG.to_radians().sin_cos();
    let eq = [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c];
    Voyager {
        au: norm(p) / AU_KM,
        speed_kms: norm(v0.vel),
        earth_au: earth_km_away / AU_KM,
        light_secs: earth_km_away / LIGHT_KMS,
        ra_hours: eq[1].atan2(eq[0]).to_degrees().rem_euclid(360.0) / 15.0,
        dec_deg: (eq[2] / norm(eq)).asin().to_degrees(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unix(jd: f64) -> f64 {
        (jd - UNIX_EPOCH_JD) * 86_400.0
    }

    #[test]
    fn at_the_epoch_it_matches_horizons() {
        let v = voyager_with(BUILT_IN, unix(EPOCH_JD));
        assert!((v.au - 172.4).abs() < 0.1, "au {}", v.au);
        assert!((v.speed_kms - 16.92).abs() < 0.02, "speed {}", v.speed_kms);
        assert!((v.ra_hours - 17.27).abs() < 0.05, "ra {}", v.ra_hours); // Ophiuchus
        assert!((v.dec_deg - 12.2).abs() < 0.2, "dec {}", v.dec_deg);
        assert!((v.light_secs / 3600.0 - 23.9).abs() < 0.3, "light {} h", v.light_secs / 3600.0);
    }

    #[test]
    fn a_day_later_matches_the_horizons_row() {
        // Horizons for 2026-11-22: x = -4.820818293e9 km (Voyager), Earth at (7.540821e7, 1.270837e8) km.
        let jd = EPOCH_JD + 1.0;
        let x = POS_KM[0] + VEL_KMS[0] * 86_400.0;
        assert!((x - -4.820_818_293e9).abs() < 1e4, "x {x}");
        let e = earth_km(jd);
        let off = ((e[0] - 7.540_821e7).powi(2) + (e[1] - 1.270_837e8).powi(2)).sqrt() / AU_KM;
        assert!(off < 0.06, "earth off by {off} AU");
    }

    #[test]
    fn it_keeps_moving_away() {
        let (a, b) = (voyager_with(BUILT_IN, unix(EPOCH_JD)), voyager_with(BUILT_IN, unix(EPOCH_JD + 365.25)));
        assert!(b.au > a.au + 3.4 && b.au < a.au + 3.7, "{} -> {}", a.au, b.au); // ~3.57 AU a year
    }

    #[test]
    fn a_horizons_answer_is_read_and_a_wrong_one_is_refused() {
        let answer = "header\n$$SOE\n2461366.500000000 = A.D. 2026-Nov-22 00:00:00.0000 TDB\n X =-4.820818293445841E+09 Y =-2.052982148090313E+10 Z = 1.485364785922957E+10\n VX=-2.069379816160355E+00 VY=-1.361137120557069E+01 VZ= 9.831446714192410E+00\n$$EOE\ntail";
        let v = parse_horizons(answer).unwrap();
        assert_eq!((v.jd, v.pos[0], v.vel[2]), (2_461_366.5, -4.820_818_293_445_841e9, 9.831_446_714_192_41));
        assert!(parse_horizons("<html>503</html>").is_none());
        assert!(parse_horizons(&answer.replace("E+10", "E+03").replace("E+09", "E+03")).is_none(), "Voyager is not a few km from the Sun");
    }
}
