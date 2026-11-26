//! The Voyager 1 page: where the probe is right now, worked out on this computer by `seiza_common::voyager`
//! from NASA's Horizons data (fetched about once a day while online, one built-in vector otherwise).
//!
//! This part is the numbers and their wording, which need no drawing to test.

use super::{Fonts, Page};
use seiza_common::{
    draw::Canvas,
    kit,
    palette::{palette, rgba},
    voyager::{light_text, voyager_at},
};

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
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
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
    ("1979-03-05", "flew past Jupiter"),
    ("1980-11-12", "flew past Saturn"),
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

pub struct Voyager {
    r: Readout,
}

impl Voyager {
    pub fn new() -> Voyager {
        let r = readout(now_secs());
        std::thread::spawn(seiza_common::voyager::refresh_if_stale); // a cache over a day old is renewed for the next opening
        Voyager { r }
    }
}

const CARD_GAP: f32 = 14.0;
const STAT_H: f32 = 104.0;

impl Page for Voyager {
    fn name(&self) -> &'static str {
        "voyager"
    }

    fn blurb(&self) -> String {
        "where Voyager 1 is right now, worked out on this computer.".into()
    }

    fn rows(&self) -> Vec<(String, String)> {
        let r = &self.r;
        let mut rows = vec![
            ("Voyager 1: distance from the sun".to_string(), format!("{} km, {:.2} AU", grouped(r.km_sun), r.au)),
            ("Voyager 1: distance from earth".to_string(), format!("{} km", grouped(r.km_earth))),
            ("Voyager 1: light time to earth".to_string(), format!("{}, {}", r.light, r.light_note)),
            ("Voyager 1: speed".to_string(), format!("{:.2} km/s, {} km/h", r.speed_kms, grouped(r.speed_kmh))),
            ("Voyager 1: direction".to_string(), format!("{} {}, toward Ophiuchus", r.ra, r.dec)),
        ];
        rows.extend(MILESTONES.iter().map(|(date, what)| (format!("Voyager 1: {date}"), what.to_string())));
        rows
    }

    fn tick(&mut self) -> bool {
        let r = readout(now_secs());
        let changed = r != self.r;
        self.r = r;
        changed
    }

    fn draw(&mut self, cv: &mut Canvas, f: &mut Fonts, scroll: f32) -> f32 {
        let (w, p, r) = (cv.w as f32, palette(), self.r.clone());
        let cols = if w >= 1000.0 { 3 } else if w >= 640.0 { 2 } else { 1 };
        let cw = (w - CARD_GAP * (cols - 1) as f32) / cols as f32;
        let stats = [
            ("from the sun", format!("{} km", grouped(r.km_sun)), format!("{:.2} AU, the farthest thing people ever made", r.au)),
            ("from earth", format!("{} km", grouped(r.km_earth)), "give or take an AU, depending on the season".to_string()),
            ("light time", r.light.clone(), format!("one way to earth, {}", r.light_note)),
            ("speed", format!("{:.2} km/s", r.speed_kms), format!("{} km/h, away from the sun", grouped(r.speed_kmh))),
            ("direction", format!("{}  {}", r.ra, r.dec), "toward the constellation Ophiuchus".to_string()),
        ];
        let rows = stats.len().div_ceil(cols);
        let ruler_y = rows as f32 * (STAT_H + CARD_GAP);
        let ruler_h = 170.0;
        let miles_y = ruler_y + ruler_h + CARD_GAP;
        let miles_h = kit::CARD_HEAD + 18.0 + MILESTONES.len() as f32 * 30.0;
        let note = kit::wrap(f.small, "worked out here from a position NASA measured (JPL Horizons, fetched about once a day while you are online; november 2026 is built in for when you are not). it flies in a straight line from there, so it stays within about 0.1 AU for ten years either side.", w);
        let height = (miles_y + miles_h + 14.0 + note.len() as f32 * 18.0 + 10.0).ceil();
        let mut c = Canvas { w: cv.w, h: height as usize, px: vec![0; cv.w * height as usize] };

        for (i, (label, value, sub)) in stats.iter().enumerate() {
            let (x, y) = ((i % cols) as f32 * (cw + CARD_GAP), (i / cols) as f32 * (STAT_H + CARD_GAP));
            kit::card(&mut c, f.small, x, y, cw, STAT_H, label);
            f.title.draw(&mut c, x + 18.0, y + kit::CARD_HEAD + 30.0, value, rgba(p.text, 1.0));
            f.small.draw(&mut c, x + 18.0, y + kit::CARD_HEAD + 52.0, sub, rgba(p.text, 0.5));
        }

        // the ruler: distance from the sun on a logarithmic scale
        kit::card(&mut c, f.small, 0.0, ruler_y, w, ruler_h, "how far, to scale (each step is a multiplication)");
        let (x0, rw, line_y) = (24.0, w - 48.0, ruler_y + kit::CARD_HEAD + 82.0);
        c.round_rect(x0, line_y - 1.0, rw, 2.0, 1.0, rgba(p.text, 0.22));
        for (i, (name, au)) in RULER[..RULER.len() - 1].iter().enumerate() {
            let x = x0 + ruler_x(*au, rw);
            let far = *name == "heliopause";
            c.round_rect(x - 1.0, line_y - 6.0, 2.0, 12.0, 1.0, rgba(if far { p.star } else { p.text }, if far { 0.9 } else { 0.45 }));
            let tw = f.small.width(name);
            let ly = if i % 2 == 0 { line_y + 24.0 } else { line_y - 14.0 };
            f.small.draw(&mut c, (x - tw / 2.0).clamp(0.0, w - tw), ly, name, rgba(p.text, if far { 0.85 } else { 0.55 }));
        }
        let vx = x0 + ruler_x(r.au, rw);
        for k in (1..=6).rev() {
            c.round_rect(vx - 5.0 * k as f32, line_y - 5.0 * k as f32, 10.0 * k as f32, 10.0 * k as f32, 5.0 * k as f32, rgba(p.star, 0.04));
        }
        c.sparkle(vx, line_y, 9.0, rgba(p.star, 1.0));
        let label = format!("Voyager 1  {:.0} AU", r.au);
        let lw = f.body.width(&label);
        f.body.draw(&mut c, (vx - lw / 2.0).min(w - lw - 8.0), line_y - 28.0, &label, rgba(p.star, 1.0));

        kit::card(&mut c, f.small, 0.0, miles_y, w, miles_h, "milestones");
        for (i, (date, what)) in MILESTONES.iter().enumerate() {
            let y = miles_y + kit::CARD_HEAD + 30.0 + i as f32 * 30.0;
            f.small.draw(&mut c, 18.0, y, date, rgba(p.text, 0.5));
            f.body.draw(&mut c, 130.0, y, what, rgba(p.text, 0.9));
        }
        for (i, line) in note.iter().enumerate() {
            f.small.draw(&mut c, 4.0, miles_y + miles_h + 24.0 + i as f32 * 18.0, line, rgba(p.text, 0.4));
        }

        cv.over(&c, 0, -(scroll as i32));
        height
    }
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

    fn draw(page: &mut Voyager, w: usize, h: usize, scroll: f32) -> (Canvas, f32) {
        use seiza_common::draw::{shared_font, Text};
        let font = shared_font();
        let (mut t, mut b, mut s) = (Text::new(font.clone(), 24.0), Text::new(font.clone(), 15.0), Text::new(font, 13.0));
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let height = page.draw(&mut cv, &mut Fonts { title: &mut t, body: &mut b, small: &mut s }, scroll);
        (cv, height)
    }

    #[test]
    fn the_page_draws_at_every_size() {
        let mut page = Voyager::new();
        let mut last = 0.0;
        for (w, h) in [(2400, 1300), (1420, 760), (900, 500), (500, 400)] {
            let (cv, height) = draw(&mut page, w, h, 0.0);
            assert!(cv.px.iter().any(|&p| p != 0) && height > 400.0, "{w}x{h}: {height}");
            assert!(height >= last || w > 1000, "narrower means taller: {height}");
            last = height;
        }
        draw(&mut page, 900, 300, 200.0); // scrolled
    }

    #[test]
    fn it_ticks_forward_and_says_what_it_found() {
        let mut page = Voyager::new();
        page.r = readout(now_secs() - 3600.0); // an hour out of date
        assert!(page.tick(), "the distance moved");
        let rows = page.rows();
        assert_eq!(rows.len(), 5 + MILESTONES.len());
        assert!(rows.iter().any(|(n, d)| n.contains("distance from the sun") && d.contains(" km, ") && d.contains("AU")));
        assert!(rows.iter().all(|(n, _)| n.starts_with("Voyager 1")), "so a search for voyager finds them all");
    }
}
