//! Where Voyager 1 is, worked out offline from one state vector.
//!
//! The vector (heliocentric, ecliptic J2000) is JPL Horizons data for 2026-11-21 00:00 TDB. Voyager
//! has escaped the Sun, so its path is a straight line to well within what a screen can show.
//! shortcut: linear motion, error under ~0.1 AU for ten years either side of EPOCH_JD; refresh the
//! constants from Horizons (COMMAND '-31', CENTER '500@10') when it should be sharper.

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

/// Voyager 1 at a Unix time in seconds (UTC; the 69 s to TDB is far below what shows).
pub fn voyager_at(unix_secs: f64) -> Voyager {
    let jd = unix_secs / 86_400.0 + UNIX_EPOCH_JD;
    let dt = (jd - EPOCH_JD) * 86_400.0;
    let p = [0, 1, 2].map(|i| POS_KM[i] + VEL_KMS[i] * dt);
    let e = earth_km(jd);
    let earth_km_away = norm([p[0] - e[0], p[1] - e[1], p[2] - e[2]]);
    // ecliptic -> equatorial: rotate about x by the obliquity. Sun-centred direction, parallax <= 0.33 deg.
    let (s, c) = OBLIQUITY_DEG.to_radians().sin_cos();
    let eq = [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c];
    Voyager {
        au: norm(p) / AU_KM,
        speed_kms: norm(VEL_KMS),
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
        let v = voyager_at(unix(EPOCH_JD));
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
        let (a, b) = (voyager_at(unix(EPOCH_JD)), voyager_at(unix(EPOCH_JD + 365.25)));
        assert!(b.au > a.au + 3.4 && b.au < a.au + 3.7, "{} -> {}", a.au, b.au); // ~3.57 AU a year
    }
}
