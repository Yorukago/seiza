//! The widgets' state and drawing: a clock with the moon's phase, a calendar and
//! the machine's pulse, as cards in a row. No Wayland here, so `--dump` can render it.

use seiza_common::{
    draw::{load_font, Canvas, Text},
    local_time,
    palette::{palette, rgba},
};

pub const STRIP_H: u32 = 330;
const GAP: f32 = 22.0;
const HISTORY: usize = 60;

/// What the system card shows; filled by the main loop once a second.
#[derive(Default, Clone, PartialEq)]
pub struct Pulse {
    pub cpu: Option<u32>,
    pub ram: Option<u32>,
    pub temp: Option<u32>,
    pub load: Option<f32>,
}

pub struct Ui {
    pub cards: Vec<String>, // "clock", "calendar", "system", in the order drawn
    pub pulse: Pulse,
    cpu_history: Vec<u32>,
    t64: Text,
    t18: Text,
    t14: Text,
}

/// Days since the first of the month's weekday (Monday = 0), for the month grid.
/// Returns the rows of day numbers (0 = blank), weeks starting on Monday.
pub fn month_grid(year: i32, month: u32, first_wday_monday0: usize) -> Vec<[u32; 7]> {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month { 2 => 28 + leap as u32, 4 | 6 | 9 | 11 => 30, _ => 31 };
    let mut rows = vec![[0u32; 7]];
    let mut col = first_wday_monday0;
    for d in 1..=days {
        if col == 7 {
            rows.push([0; 7]);
            col = 0;
        }
        rows.last_mut().unwrap()[col] = d;
        col += 1;
    }
    rows
}

/// The moon's age in days (0 = new), from the mean synodic month.
pub fn moon_age(unix_secs: f64) -> f64 {
    const PERIOD: f64 = 29.530588853;
    const NEW_MOON_2000: f64 = 947_182_440.0; // 2000-01-06 18:14 UTC
    ((unix_secs - NEW_MOON_2000) / 86_400.0).rem_euclid(PERIOD)
}

fn moon_name(age: f64) -> &'static str {
    match age {
        a if !(1.5..28.0).contains(&a) => "new moon",
        a if a < 6.5 => "waxing crescent",
        a if a < 8.5 => "first quarter",
        a if a < 13.5 => "waxing gibbous",
        a if a < 16.0 => "full moon",
        a if a < 21.0 => "waning gibbous",
        a if a < 23.0 => "last quarter",
        _ => "waning crescent",
    }
}

fn now_parts() -> (i32, u32, u32, usize, f64) {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        (tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32, ((tm.tm_wday + 6) % 7) as usize, t as f64)
    }
}

impl Ui {
    pub fn new(cards: Vec<String>) -> Ui {
        Ui { cards, pulse: Pulse::default(), cpu_history: vec![], t64: Text::new(load_font(), 64.0), t18: Text::new(load_font(), 18.0), t14: Text::new(load_font(), 14.0) }
    }

    pub fn set_pulse(&mut self, p: Pulse) {
        self.cpu_history.push(p.cpu.unwrap_or(0));
        if self.cpu_history.len() > HISTORY {
            self.cpu_history.remove(0);
        }
        self.pulse = p;
    }

    /// Redraw when this changes.
    pub fn key(&self) -> String {
        format!("{}|{}|{}", local_time(c"%Y%m%d%H%M"), self.cpu_history.len(), self.cpu_history.last().copied().unwrap_or(0))
    }

    fn card_size(name: &str) -> (f32, f32) {
        match name {
            "clock" => (330.0, 150.0),
            "calendar" => (340.0, 300.0),
            _ => (340.0, 236.0),
        }
    }

    pub fn width(&self) -> u32 {
        let cards: Vec<_> = self.cards.iter().filter(|c| ["clock", "calendar", "system"].contains(&c.as_str())).collect();
        (cards.iter().map(|c| Self::card_size(c).0 + GAP).sum::<f32>() - GAP).max(1.0) as u32
    }

    pub fn render(&mut self, w: usize, h: usize) -> Canvas {
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let cards: Vec<String> = self.cards.iter().filter(|c| ["clock", "calendar", "system"].contains(&c.as_str())).cloned().collect();
        let mut x = (w as f32 - self.width() as f32) / 2.0;
        for name in cards {
            let (cw, ch) = Self::card_size(&name);
            let p = palette();
            cv.round_rect(x - 1.0, 0.0, cw + 2.0, ch + 1.0, 23.0, rgba(p.star, 0.16));
            cv.round_rect(x, 1.0, cw, ch - 1.0, 22.0, rgba(p.panel, 0.80));
            match name.as_str() {
                "clock" => self.clock(&mut cv, x, cw, ch),
                "calendar" => self.calendar(&mut cv, x, cw),
                _ => self.system(&mut cv, x, cw),
            }
            x += cw + GAP;
        }
        cv
    }

    fn clock(&mut self, cv: &mut Canvas, x: f32, cw: f32, ch: f32) {
        let p = palette();
        let (_, _, _, _, t) = now_parts();
        self.t64.draw(cv, x + 26.0, 82.0, &local_time(c"%H:%M"), rgba(p.text, 1.0));
        self.t18.draw(cv, x + 28.0, 116.0, &local_time(c"%a %d %b").to_lowercase(), rgba(p.text, 0.6));
        // the moon, drawn dim as a whole disc with the lit part over it
        let (mx, my, r) = (x + cw - 52.0, 56.0, 20.0);
        let age = moon_age(t);
        cv.shape(mx, my, r, rgba(p.text, 0.14), |px, py| (px - mx).hypot(py - my) <= r);
        let k = (std::f64::consts::TAU * age / 29.530588853).cos() as f32;
        cv.shape(mx, my, r, rgba(p.text, 0.95), |px, py| {
            let (ux, uy) = ((px - mx) / r, (py - my) / r);
            let edge = k * (1.0 - uy * uy).max(0.0).sqrt();
            ux * ux + uy * uy <= 1.0 && if age < 14.765 { ux > edge } else { ux < -edge }
        });
        let name = moon_name(age);
        let nw = self.t14.width(name);
        self.t14.draw(cv, x + cw - 24.0 - nw, ch - 24.0, name, rgba(p.text, 0.5));
    }

    fn calendar(&mut self, cv: &mut Canvas, x: f32, cw: f32) {
        let p = palette();
        let (year, month, today, wday, _) = now_parts();
        let first = (wday + 7 - ((today as usize - 1) % 7)) % 7;
        let title = local_time(c"%B %Y").to_lowercase();
        self.t18.draw(cv, x + 24.0, 40.0, &title, rgba(p.text, 1.0));
        let col_w = (cw - 48.0) / 7.0;
        for (i, d) in ["m", "t", "w", "t", "f", "s", "s"].iter().enumerate() {
            let dw = self.t14.width(d);
            self.t14.draw(cv, x + 24.0 + col_w * (i as f32 + 0.5) - dw / 2.0, 72.0, d, rgba(p.text, 0.4));
        }
        for (r, row) in month_grid(year, month, first).iter().enumerate() {
            for (c, &d) in row.iter().enumerate() {
                if d == 0 {
                    continue;
                }
                let (cx, cy) = (x + 24.0 + col_w * (c as f32 + 0.5), 104.0 + r as f32 * 31.0);
                let s = d.to_string();
                let sw = self.t14.width(&s);
                if d == today {
                    cv.round_rect(cx - 14.0, cy - 14.0, 28.0, 28.0, 14.0, rgba(p.star, 0.9));
                    self.t14.draw(cv, cx - sw / 2.0, cy + 5.0, &s, rgba(p.sky, 1.0)); // dark on gold: never gold under text
                } else {
                    self.t14.draw(cv, cx - sw / 2.0, cy + 5.0, &s, rgba(p.text, if c >= 5 { 0.55 } else { 0.85 }));
                }
            }
        }
    }

    fn system(&mut self, cv: &mut Canvas, x: f32, cw: f32) {
        let p = palette();
        self.t14.draw(cv, x + 24.0, 38.0, "system", rgba(p.text, 0.5));
        let rows: [(&str, Option<u32>, String); 3] = [
            ("cpu", self.pulse.cpu, self.pulse.cpu.map_or("-".into(), |v| format!("{v}%"))),
            ("ram", self.pulse.ram, self.pulse.ram.map_or("-".into(), |v| format!("{v}%"))),
            ("temp", self.pulse.temp.map(|t| t.min(100)), self.pulse.temp.map_or("-".into(), |v| format!("{v}°C"))),
        ];
        for (i, (name, frac, text)) in rows.iter().enumerate() {
            let y = 72.0 + i as f32 * 34.0;
            self.t14.draw(cv, x + 24.0, y + 5.0, name, rgba(p.text, 0.7));
            let (bx, bw) = (x + 80.0, cw - 80.0 - 78.0);
            cv.round_rect(bx, y - 3.0, bw, 6.0, 3.0, rgba(p.dust, 0.9));
            if let Some(f) = frac {
                cv.round_rect(bx, y - 3.0, (bw * *f as f32 / 100.0).max(6.0), 6.0, 3.0, rgba(p.star, 0.9));
            }
            let tw = self.t14.width(text);
            self.t14.draw(cv, x + cw - 24.0 - tw, y + 5.0, text, rgba(p.text, 0.9));
        }
        // the last minute of cpu, as an inked line
        let (lx, ly, lw, lh) = (x + 24.0, 176.0, cw - 48.0, 40.0);
        let n = self.cpu_history.len();
        for i in 1..n {
            let pt = |j: usize| (lx + lw * j as f32 / (HISTORY - 1) as f32, ly + lh - lh * self.cpu_history[j] as f32 / 100.0);
            let ((x0, y0), (x1, y1)) = (pt(i - 1), pt(i));
            for s in 0..8 {
                let t = s as f32 / 8.0;
                let (px, py) = (x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
                cv.round_rect(px - 0.8, py - 0.8, 1.6, 1.6, 0.8, rgba(p.star, 0.8));
            }
        }
        if n == 0 {
            self.t14.draw(cv, lx, ly + lh / 2.0, "listening..", rgba(p.text, 0.35));
        }
        if let Some(l) = self.pulse.load {
            let s = format!("load {l:.2}");
            let sw = self.t14.width(&s);
            self.t14.draw(cv, x + cw - 24.0 - sw, 38.0, &s, rgba(p.text, 0.4));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn october_2026_starts_on_thursday_and_has_31_days() {
        let g = month_grid(2026, 10, 3);
        assert_eq!(g[0], [0, 0, 0, 1, 2, 3, 4]);
        assert_eq!(g.iter().flatten().copied().max(), Some(31));
        assert_eq!(g.len(), 5);
    }

    #[test]
    fn february_knows_leap_years() {
        assert_eq!(month_grid(2024, 2, 3).iter().flatten().copied().max(), Some(29));
        assert_eq!(month_grid(2026, 2, 0).iter().flatten().copied().max(), Some(28));
        assert_eq!(month_grid(1900, 2, 0).iter().flatten().copied().max(), Some(28));
    }

    #[test]
    fn the_moon_is_full_on_a_known_full_moon_day() {
        // 2026-10-26 is a full moon (about 04:12 UTC)
        let age = moon_age(1_792_988_000.0);
        assert!((age - 14.77).abs() < 1.0, "age {age}");
        assert_eq!(moon_name(age), "full moon");
    }
}
