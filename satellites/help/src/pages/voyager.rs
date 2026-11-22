//! The Voyager 1 page: where the probe is right now, worked out offline by `seiza_common::voyager`.
//!
//! This part is the numbers and their wording, which need no drawing to test.

use seiza_common::voyager::voyager_at;

const AU_KM: f64 = 149_597_870.7;
const LIGHT_DAY_SECS: f64 = 86_400.0;

/// Everything the page shows that changes with the clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Readout {
    pub km_sun: u64,
    pub au: f64,
    pub km_earth: u64,
    pub light: String, // one-way light time to Earth, "23 h 58 min"
    pub light_note: &'static str,
    pub speed_kms: f64,
    pub speed_kmh: u64,
    pub ra: String,
    pub dec: String,
}

pub fn now_secs() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

/// 25790123456 -> "25,790,123,456".
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A light time as hours and minutes, or minutes and seconds under an hour.
pub fn light_text(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    match total {
        0..=3599 => format!("{} min {:02} s", total / 60, total % 60),
        _ => format!("{} h {:02} min", total / 3600, total % 3600 / 60),
    }
}

/// Right ascension in hours as "17h 16m".
pub fn ra_text(hours: f64) -> String {
    let m = (hours.rem_euclid(24.0) * 60.0).round() as u32 % 1440;
    format!("{}h {:02}m", m / 60, m % 60)
}

/// Declination as "+12.2°".
pub fn dec_text(deg: f64) -> String {
    format!("{deg:+.1}°")
}

pub fn readout(unix_secs: f64) -> Readout {
    let v = voyager_at(unix_secs);
    Readout {
        km_sun: (v.au * AU_KM).round() as u64,
        au: v.au,
        km_earth: (v.earth_au * AU_KM).round() as u64,
        light: light_text(v.light_secs),
        light_note: if v.light_secs >= LIGHT_DAY_SECS { "more than a light-day away" } else { "almost a light-day away" },
        speed_kms: v.speed_kms,
        speed_kmh: (v.speed_kms * 3600.0).round() as u64,
        ra: ra_text(v.ra_hours),
        dec: dec_text(v.dec_deg),
    }
}

/// (date, what happened), oldest first.
pub const MILESTONES: [(&str, &str); 5] = [
    ("1977-09-05", "launched from Cape Canaveral"),
    ("1979-03-05", "closest approach to Jupiter"),
    ("1980-11-12", "closest approach to Saturn"),
    ("1990-02-14", "looked back and took the Pale Blue Dot portrait of Earth"),
    ("2012-08-25", "crossed the heliopause into interstellar space"),
];

/// Landmarks on the distance ruler, in AU from the Sun.
pub const RULER: [(&str, f64); 8] = [("Earth", 1.0), ("Jupiter", 5.2), ("Saturn", 9.58), ("Uranus", 19.2), ("Neptune", 30.07), ("Pluto", 39.5), ("heliopause", 121.6), ("Voyager 1", 0.0)];

const RULER_FROM: f64 = 0.6;
const RULER_TO: f64 = 260.0;

/// Where `au` falls on a ruler `width` long. The scale is logarithmic, or the planets would all sit on the Sun.
pub fn ruler_x(au: f64, width: f32) -> f32 {
    let t = (au.max(RULER_FROM).ln() - RULER_FROM.ln()) / (RULER_TO.ln() - RULER_FROM.ln());
    (t.clamp(0.0, 1.0) as f32) * width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_grouped_by_thousands() {
        assert_eq!([grouped(0), grouped(999), grouped(1000), grouped(25_790_123_456), grouped(1_000_000)], ["0", "999", "1,000", "25,790,123,456", "1,000,000"]);
    }

    #[test]
    fn light_time_reads_naturally() {
        assert_eq!(light_text(86_400.0), "24 h 00 min");
        assert_eq!(light_text(86_400.0 + 59.0 * 60.0 + 31.0), "24 h 59 min");
        assert_eq!(light_text(499.0), "8 min 19 s");
        assert_eq!(light_text(-5.0), "0 min 00 s");
    }

    #[test]
    fn the_sky_position_is_written_as_hours_and_degrees() {
        assert_eq!(ra_text(17.27), "17h 16m");
        assert_eq!(ra_text(23.999), "0h 00m", "rounds up across midnight");
        assert_eq!(ra_text(-0.5), "23h 30m");
        assert_eq!((dec_text(12.2), dec_text(-3.04)), ("+12.2°".to_string(), "-3.0°".to_string()));
    }

    #[test]
    fn the_readout_agrees_with_the_known_position() {
        // 2026-11-21 00:00 UTC, the epoch the constants come from
        let r = readout((2_461_365.5 - 2_440_587.5) * 86_400.0);
        assert!((r.au - 172.4).abs() < 0.1, "{}", r.au);
        assert_eq!(r.km_sun / 1_000_000_000, 25, "about 25.8 billion km");
        assert!((r.speed_kmh as f64 - 60_900.0).abs() < 100.0, "{}", r.speed_kmh);
        assert_eq!(r.ra, "17h 16m");
        assert!(r.km_earth.abs_diff(r.km_sun) < 160_000_000, "Earth is within an AU of the Sun");
    }

    #[test]
    fn the_light_day_note_follows_the_light_time() {
        let early = readout((2_461_365.5 - 2_440_587.5) * 86_400.0 - 400.0 * 86_400.0);
        let late = readout((2_461_365.5 - 2_440_587.5) * 86_400.0 + 400.0 * 86_400.0);
        assert_eq!((early.light_note, late.light_note), ("almost a light-day away", "more than a light-day away"));
    }

    #[test]
    fn the_distance_keeps_growing_between_ticks() {
        let t = now_secs();
        let (a, b) = (readout(t), readout(t + 60.0));
        let moved = b.km_sun - a.km_sun;
        assert!((900..1100).contains(&moved), "about 17 km a second, so about 1,000 km a minute: {moved}");
    }

    #[test]
    fn milestones_are_in_order() {
        assert!(MILESTONES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn the_ruler_is_logarithmic_and_ordered() {
        let w = 1000.0;
        assert_eq!((ruler_x(0.1, w), ruler_x(1000.0, w)), (0.0, w), "off the ends it clamps");
        let xs: Vec<f32> = RULER[..7].iter().map(|r| ruler_x(r.1, w)).collect();
        assert!(xs.windows(2).all(|p| p[0] < p[1]), "{xs:?}");
        assert!(xs[1] - xs[0] > 100.0 && xs[6] - xs[5] > 100.0, "the planets and the far landmarks both have room");
    }
}
