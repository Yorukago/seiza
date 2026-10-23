//! The lock screen's drawing: a night sky with a moon, the clock, and the
//! password as stars on an inked line. No Wayland here, so `--dump` can render it.

use std::time::Instant;

use seiza_common::{
    draw::{blend, load_font, Canvas, Rgba, Text},
    local_time,
    palette::{palette, rgba},
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Idle,
    Checking,
    Wrong,
}

pub struct Ui {
    big: Text,
    small: Text,
    sky: Option<Canvas>,              // the sky for the current size; it never changes while locked
    base: Option<(String, Canvas)>,   // sky + clock + quip, and what it was drawn for
    start: Instant,
    last: f32,                        // when the animation state was last advanced, seconds since start
    births: Vec<f32>,                 // when each typed star appeared
    dying: Vec<(usize, f32)>,         // stars just deleted: their slot and when
    wrong_at: Option<f32>,
    prev_status: Status,
    spin: f32,                        // 1 while the field is empty: the strands twist; eases to 0 once you type
    phase: f32,
    shown_n: f32,                     // how many slots the line spreads over, easing toward the real number
    busy: bool,
    lines: bool, // Yoru's lines (the words under the password): `lines` in ~/.config/seiza/yoru
    fast: bool,                       // something quick is moving (a pop, a shake): worth every frame the screen has
}

fn hash(n: u64) -> u64 {
    let mut h = n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD6E8_FEB8_6659_FD93;
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^ (h >> 32)
}

fn unit(h: u64, shift: u32) -> f32 {
    ((h >> shift) & 0xFFFF) as f32 / 65535.0
}

/// The soft glow used around the moon and stars: one smooth pass, fading out over four radii.
fn glow(cv: &mut Canvas, x: f32, y: f32, r: f32, c: Rgba) {
    let reach = r * 4.0;
    let (x0, x1) = ((x - reach).max(0.0) as usize, ((x + reach) as usize + 1).min(cv.w));
    let (y0, y1) = ((y - reach).max(0.0) as usize, ((y + reach) as usize + 1).min(cv.h));
    for py in y0..y1 {
        for px in x0..x1 {
            let t = 1.0 - (px as f32 + 0.5 - x).hypot(py as f32 + 0.5 - y) / reach;
            if t > 0.0 {
                blend(&mut cv.px[py * cv.w + px], c, 0.6 * t * t);
            }
        }
    }
}

fn draw_sky(w: usize, h: usize) -> Canvas {
    let p = palette();
    let mut cv = Canvas { w, h, px: vec![0xFF00_0000 | ch(p.sky[0]) << 16 | ch(p.sky[1]) << 8 | ch(p.sky[2]); w * h] };
    let (fw, fh) = (w as f32, h as f32);
    // a few big, dim clouds of dust and mist
    for i in 0..6u64 {
        let hv = hash(i + 7);
        let (x, y, r) = (unit(hv, 0) * fw, unit(hv, 16) * fh, fh * (0.18 + unit(hv, 32) * 0.2));
        glow(&mut cv, x, y, r * 0.5, rgba(if i % 2 == 0 { p.dust } else { p.mist }, 0.55));
    }
    // scattered stars, each its own size and brightness
    for i in 0..70u64 {
        let hv = hash(i + 100);
        let (x, y, r) = (unit(hv, 0) * fw, unit(hv, 16) * fh, 1.5 + unit(hv, 32) * 3.5);
        glow(&mut cv, x, y, r, rgba(p.star, 0.35));
        cv.sparkle(x, y, r, rgba(p.star, 0.4 + unit(hv, 48) * 0.5));
    }
    // the moon, with a star keeping it company; it never changes, so it is part of the sky
    let (cx, mr, my) = (fw / 2.0, fh * 0.11, fh * 0.27);
    glow(&mut cv, cx, my, mr, rgba(p.text, 0.5));
    cv.crescent(cx, my, mr, rgba(p.text, 1.0));
    cv.sparkle(cx + mr * 1.15, my - mr * 0.1, mr * 0.2, rgba(p.star, 1.0));
    cv
}

fn ch(v: f32) -> u32 {
    (v * 255.0) as u32
}

/// Overshoots well past 1 before settling: a star popping up.
fn bloom(t: f32) -> f32 {
    let (c1, c3) = (3.2f32, 4.2f32);
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + c3 * t * t * t + c1 * t * t
}

impl Ui {
    pub fn new() -> Ui {
        let font = load_font(); // looking one up runs a subprocess, so once
        Ui {
            big: Text::new(font.clone(), 112.0),
            small: Text::new(font, 18.0),
            sky: None,
            base: None,
            start: Instant::now(),
            last: 0.0,
            births: vec![],
            dying: vec![],
            wrong_at: None,
            prev_status: Status::Idle,
            spin: 1.0,
            phase: 0.0,
            shown_n: 3.0,
            busy: true,
            lines: seiza_common::config::flag("yoru", "lines", true),
            fast: false,
        }
    }

    /// What decides whether a redraw is needed even when nothing is animating.
    pub fn key(&self, typed: usize, status: Status) -> String {
        format!("{}|{typed}|{status:?}", local_time(seiza_common::clock_fmt()))
    }

    /// True while something on screen is moving by itself, so the caller should keep redrawing.
    pub fn busy(&self) -> bool {
        self.busy
    }

    /// True while that movement is quick; the slow twist of an empty field is fine at half the frames.
    pub fn fast(&self) -> bool {
        self.fast
    }

    fn centered(t: &mut Text, cv: &mut Canvas, cx: f32, base: f32, s: &str, c: Rgba) {
        let w = t.width(s);
        t.draw(cv, cx - w / 2.0, base, s, c);
    }

    /// Move the animation state on to now, given what is typed.
    fn advance(&mut self, typed: usize, status: Status) -> f32 {
        let now = self.start.elapsed().as_secs_f32();
        let dt = (now - self.last).clamp(0.0, 0.1);
        self.last = now;
        while self.births.len() < typed {
            self.births.push(now);
        }
        while self.births.len() > typed {
            self.dying.push((self.births.len() - 1, now));
            self.births.pop();
        }
        self.dying.retain(|d| now - d.1 < 0.25);
        if status == Status::Wrong && self.prev_status != Status::Wrong {
            self.wrong_at = Some(now);
        }
        self.prev_status = status;
        self.spin += ((if typed == 0 { 1.0 } else { 0.0 }) - self.spin) * (1.0 - (-dt * 3.0).exp());
        self.phase += dt * (0.1 + 0.4 * self.spin); // very slowly: about 12 s for a full twist while empty
        let want = typed.max(3) as f32;
        self.shown_n += (want - self.shown_n) * (1.0 - (-dt * 10.0).exp());
        self.fast = status == Status::Checking
            || self.births.last().is_some_and(|b| now - b < 0.65)
            || !self.dying.is_empty()
            || self.wrong_at.is_some_and(|w| now - w < 0.7)
            || (self.shown_n - want).abs() > 0.01;
        self.busy = typed == 0 || self.fast;
        now
    }

    pub fn render(&mut self, w: usize, h: usize, typed: usize, status: Status) -> Canvas {
        let now = self.advance(typed, status);
        if self.sky.as_ref().is_none_or(|s| s.w != w || s.h != h) {
            self.sky = Some(draw_sky(w, h));
        }
        let p = palette();
        let (fw, fh, cx) = (w as f32, h as f32, w as f32 / 2.0);

        // the part that changes only with the minute or the words: the clock and what Seiza says
        let quip = match status {
            _ if !self.lines => "", // Yoru's lines are switched off in the settings
            Status::Idle if typed == 0 => "welcome back. i didn't touch anything..",
            Status::Idle => "",
            Status::Checking => "one sec..",
            Status::Wrong => "hm. that's not it.. try again?",
        };
        let key = format!("{w}x{h}|{}|{quip}", local_time(seiza_common::clock_fmt()));
        if self.base.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut cv = Canvas { w, h, px: self.sky.as_ref().unwrap().px.clone() };
            Self::centered(&mut self.big, &mut cv, cx, fh * 0.55, &local_time(seiza_common::clock_fmt()), rgba(p.text, 1.0));
            Self::centered(&mut self.small, &mut cv, cx, fh * 0.55 + 36.0, &local_time(c"%a %d %b").to_lowercase(), rgba(p.text, 0.6));
            Self::centered(&mut self.small, &mut cv, cx, fh * 0.70 + 56.0 + 44.0, quip, rgba(p.text, 0.7));
            self.base = Some((key, cv));
        }
        let mut cv = Canvas { w, h, px: self.base.as_ref().unwrap().1.px.clone() };

        // the password: two inked strands that twist round each other while the field is empty,
        // and a star for each character, blooming in as it is typed
        let shake = self.wrong_at.map_or(0.0, |t| {
            let a = now - t;
            if a < 0.7 { 9.0 * (-6.0 * a).exp() * (a * 38.0).sin() } else { 0.0 }
        });
        let (pw, ph) = (fw.min(420.0), 56.0);
        let (px, py) = (cx - pw / 2.0 + shake, fh * 0.70);
        cv.round_rect(px - 1.0, py - 1.0, pw + 2.0, ph + 2.0, ph / 2.0 + 1.0, rgba(if status == Status::Wrong { p.pink } else { p.star }, 0.35));
        cv.round_rect(px, py, pw, ph, ph / 2.0, rgba(p.panel, 1.0));
        let (x0, x1, ly) = (px + 34.0, px + pw - 34.0, py + ph / 2.0);
        let amp = 2.0 + 3.0 * self.spin;
        for pass in 0..2 {
            let off = pass as f32 * std::f32::consts::PI;
            let mut x = x0;
            while x < x1 {
                let th = (x - x0) * 0.045 + self.phase + off;
                let depth = th.cos() * 0.5 + 0.5; // the strand coming toward you is brighter and thicker
                let r = 0.55 + 0.5 * depth;
                cv.round_rect(x - r, ly + amp * th.sin() - r, r * 2.0, r * 2.0, r, rgba(p.star, 0.25 + 0.5 * depth));
                x += 1.4;
            }
        }
        let step = (x1 - x0) / (self.shown_n.max(2.0) - 1.0);
        let star = |cv: &mut Canvas, i: usize, scale: f32, alpha: f32, rise: f32| {
            let hv = hash(i as u64 * 31 + 5);
            let r = (8.0 + unit(hv, 0) * 4.0) * scale * (1.0 + 0.06 * (now * 1.3 + i as f32 * 1.7).sin());
            let (x, y) = (x0 + step * i as f32, ly + rise);
            if r > 0.5 {
                glow(cv, x, y, r, rgba(p.star, 0.7 * alpha));
                cv.sparkle(x, y, r, rgba(p.star, alpha));
            }
        };
        for (i, &born) in self.births.iter().enumerate() {
            let age = now - born;
            let wave = if status == Status::Checking { 1.0 + 0.22 * (now * 7.0 - i as f32 * 0.7).sin() } else { 1.0 };
            let rise = 16.0 * (1.0 - (age / 0.3).min(1.0)).powi(2); // pops up from below the line
            star(&mut cv, i, bloom(age / 0.55) * wave, (age / 0.12).min(1.0), rise);
            if age < 0.6 {
                // a ring spreading out, and a few sparks flying off in directions of their own
                let (x, y) = (x0 + step * i as f32, ly + rise);
                let t = age / 0.6;
                let rr = 8.0 + 34.0 * t;
                cv.round_rect(x - rr, y - rr, rr * 2.0, rr * 2.0, rr, rgba(p.star, 0.22 * (1.0 - t)));
                cv.round_rect(x - rr + 1.5, y - rr + 1.5, rr * 2.0 - 3.0, rr * 2.0 - 3.0, rr - 1.5, rgba(p.panel, 0.22 * (1.0 - t)));
                for k in 0..6 {
                    let ang = hash(i as u64 * 7 + k).wrapping_mul(0x9E37) as f32 % 628.0 / 100.0;
                    let d = 10.0 + 30.0 * (1.0 - (1.0 - t).powi(2));
                    cv.sparkle(x + ang.cos() * d, y + ang.sin() * d, 2.6 * (1.0 - t), rgba(p.text, 0.9 * (1.0 - t)));
                }
            }
        }
        for &(i, since) in &self.dying {
            let t = 1.0 - (now - since) / 0.25;
            star(&mut cv, i, t, t, 0.0);
        }
        if typed == 0 {
            for i in 0..3 {
                let x = x0 + (x1 - x0) * i as f32 / 2.0;
                cv.sparkle(x, ly, 3.0, rgba(p.text, 0.22 + 0.12 * (now * 0.8 + i as f32 * 2.0).sin()));
            }
        }

        cv.px.iter_mut().for_each(|px| *px |= 0xFF00_0000); // blending rounds alpha down to 254; the sky must be opaque
        cv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_screen_is_fully_opaque() {
        let mut ui = Ui::new();
        for (typed, st) in [(0, Status::Idle), (5, Status::Checking), (3, Status::Wrong)] {
            let cv = ui.render(640, 360, typed, st);
            assert!(cv.px.iter().all(|p| p >> 24 == 0xFF), "a see-through pixel would show the session behind the lock");
        }
    }
}
