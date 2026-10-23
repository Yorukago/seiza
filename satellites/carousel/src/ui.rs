//! The constellation view: a film strip with one card per constellation (the one you are in
//! in the middle, the others smaller to each side) and an "add custom?" card at the end.
//! No Wayland here, so the tests and `--dump` can run it.

use std::time::{Duration, Instant};

use seiza_common::{
    constellation::Constellation,
    draw::{blend, load_font, Canvas, Rgba, Text},
    palette::{palette, rgba},
};

const SLIDE: Duration = Duration::from_millis(280);
const SIDE_SCALE: f32 = 0.60;
const STAR_REACH: f32 = 20.0; // how near a click must be to a star on the middle card, in pixels

pub enum Effect {
    None,
    /// Go into a star of a constellation (loading the constellation first if it is not the open one).
    Open { name: String, star: usize },
    /// Make a new constellation with this name, and go into it.
    Create(String),
}

pub struct Ui {
    pub cards: Vec<Constellation>,
    pub current: String,      // the open constellation
    pub current_star: String, // the star you are in
    pub sel: usize,           // the middle card; `cards.len()` is the "add custom?" card
    slide: Option<(f32, Instant)>, // where the strip was, and when it started to move
    pub name: String,         // being typed on the add card
    pub msg: String,
    thumbs: Vec<Canvas>, // each card, drawn once at its full size
    thumbs_for: (usize, usize),
    t30: Text,
    t18: Text,
    t13: Text,
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

/// Layout, all from the screen size: the strip, the card, where the middle of things is.
#[derive(Clone, Copy)]
pub struct Layout {
    pub w: f32,
    pub h: f32,
    pub card_w: f32,
    pub card_h: f32,
    pub cy: f32,
    pub band_top: f32,
    pub band_bottom: f32,
}

impl Layout {
    pub fn new(w: f32, h: f32) -> Layout {
        let card_h = (h * 0.70).min(w * 0.62);
        Layout { w, h, card_w: card_h * 1.18, card_h, cy: h * 0.495, band_top: h * 0.10, band_bottom: h * 0.89 }
    }

    /// Where a card `d` places away from the middle sits, how big it is drawn, how clear.
    fn place(&self, d: f32) -> (f32, f32, f32) {
        let a = d.abs();
        let first = self.card_w * (1.0 + SIDE_SCALE) / 2.0 + self.w * 0.045; // middle to the next card
        let x = if a <= 1.0 { a * first } else { first + (a - 1.0) * (self.card_w * SIDE_SCALE + self.w * 0.045) };
        let scale = 1.0 - (1.0 - SIDE_SCALE) * a.min(1.0);
        let alpha = (1.0 - 0.5 * a.min(1.0)) * (1.0 - ((a - 1.0).max(0.0) * 0.8)).max(0.0);
        (self.w / 2.0 + d.signum() * x, scale, alpha)
    }
}

/// Draw the sprite `src` onto `dst` with its middle at (cx, cy), scaled by `s`, at opacity `a`
/// (bilinear, over, premultiplied on both sides).
fn blit(dst: &mut Canvas, src: &Canvas, cx: f32, cy: f32, s: f32, a: f32) {
    let (sw, sh) = (src.w as f32 * s, src.h as f32 * s);
    let (x0, y0) = (cx - sw / 2.0, cy - sh / 2.0);
    let at = |x: i32, y: i32| -> [f32; 4] {
        if x < 0 || y < 0 || x >= src.w as i32 || y >= src.h as i32 {
            return [0.0; 4];
        }
        let p = src.px[y as usize * src.w + x as usize];
        [(p >> 24) as f32, (p >> 16 & 255) as f32, (p >> 8 & 255) as f32, (p & 255) as f32]
    };
    for y in (y0.floor().max(0.0) as usize)..(((y0 + sh).ceil() as usize) + 1).min(dst.h) {
        for x in (x0.floor().max(0.0) as usize)..(((x0 + sw).ceil() as usize) + 1).min(dst.w) {
            let (fx, fy) = ((x as f32 + 0.5 - x0) / s - 0.5, (y as f32 + 0.5 - y0) / s - 0.5);
            let (ix, iy, tx, ty) = (fx.floor() as i32, fy.floor() as i32, fx - fx.floor(), fy - fy.floor());
            let (p00, p10, p01, p11) = (at(ix, iy), at(ix + 1, iy), at(ix, iy + 1), at(ix + 1, iy + 1));
            let m = |i: usize| (p00[i] * (1.0 - tx) + p10[i] * tx) * (1.0 - ty) + (p01[i] * (1.0 - tx) + p11[i] * tx) * ty;
            let sa = m(0) * a / 255.0;
            if sa <= 0.0 {
                continue;
            }
            let d = dst.px[y * dst.w + x];
            let ch = |shift: u32, v: f32| ((d >> shift & 255) as f32 * (1.0 - sa) + v * a).min(255.0) as u32;
            dst.px[y * dst.w + x] = ((sa * 255.0).min(255.0) as u32 + ((d >> 24 & 255) as f32 * (1.0 - sa)) as u32).min(255) << 24 | ch(16, m(1)) << 16 | ch(8, m(2)) << 8 | ch(0, m(3));
        }
    }
}

/// Soft glow.
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

/// An inked thread: a gentle arc drawn twice, a little wobbly.
fn thread(cv: &mut Canvas, a: (f32, f32), b: (f32, f32), seed: usize, c: Rgba) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy).max(1.0);
    let bend = len * (0.06 + (seed % 4) as f32 * 0.015) * if seed % 2 == 0 { 1.0 } else { -1.0 };
    let ctrl = ((a.0 + b.0) / 2.0 - dy / len * bend, (a.1 + b.1) / 2.0 + dx / len * bend);
    for pass in 0..2 {
        let steps = (len / 1.6).ceil() as i32;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let (u, v) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t));
            let wob = if pass == 0 { 0.0 } else { 1.6 * (t * 9.0 + seed as f32).sin() + 1.3 };
            let (x, y) = (u * a.0 + v * ctrl.0 + t * t * b.0 - dy / len * wob, u * a.1 + v * ctrl.1 + t * t * b.1 + dx / len * wob);
            cv.round_rect(x - 0.9, y - 0.9, 1.8, 1.8, 0.9, [c[0], c[1], c[2], c[3] * if pass == 0 { 1.0 } else { 0.4 }]);
        }
    }
}

/// Cut the canvas to a rounded rectangle: whatever is outside the corners goes clear.
fn clip_round(cv: &mut Canvas, r: f32) {
    let (w, h) = (cv.w as f32, cv.h as f32);
    for py in 0..cv.h {
        for px in 0..cv.w {
            let (dx, dy) = ((px as f32 + 0.5 - w / 2.0).abs() - (w / 2.0 - r), (py as f32 + 0.5 - h / 2.0).abs() - (h / 2.0 - r));
            if dx > 0.0 && dy > 0.0 {
                let d = dx.hypot(dy) - r;
                let cover = (0.5 - d).clamp(0.0, 1.0);
                if cover < 1.0 {
                    let p = &mut cv.px[py * cv.w + px];
                    let ch = |s: u32| (((*p >> s) & 0xFF) as f32 * cover) as u32;
                    *p = ch(24) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0);
                }
            }
        }
    }
}

impl Ui {
    pub fn new(mut cards: Vec<Constellation>, current: String, current_star: String) -> Ui {
        cards.sort_by(|a, b| a.name.cmp(&b.name));
        let sel = cards.iter().position(|c| c.name == current).unwrap_or(0);
        let font = load_font();
        Ui {
            cards,
            current,
            current_star,
            sel,
            slide: None,
            name: String::new(),
            msg: String::new(),
            thumbs: vec![],
            thumbs_for: (0, 0),
            t30: Text::new(font.clone(), 30.0),
            t18: Text::new(font.clone(), 18.0),
            t13: Text::new(font, 13.0),
        }
    }

    pub fn len(&self) -> usize {
        self.cards.len() + 1 // the add card
    }

    fn add_card(&self) -> usize {
        self.cards.len()
    }

    /// The strip's position in cards, easing toward the selected one.
    fn pos(&self) -> f32 {
        let Some((from, t0)) = self.slide else { return self.sel as f32 };
        let t = (t0.elapsed().as_secs_f32() / SLIDE.as_secs_f32()).min(1.0);
        from + (self.sel as f32 - from) * (1.0 - (1.0 - t).powi(3))
    }

    pub fn sliding(&self) -> bool {
        self.slide.is_some_and(|(_, t0)| t0.elapsed() < SLIDE)
    }

    pub fn select(&mut self, i: usize) {
        let i = i.min(self.len() - 1);
        if i != self.sel {
            self.slide = Some((self.pos(), Instant::now()));
            self.sel = i;
            self.msg.clear();
        }
    }

    pub fn step(&mut self, d: i32) {
        self.select((self.sel as i32 + d).clamp(0, self.len() as i32 - 1) as usize);
    }

    pub fn type_text(&mut self, s: &str) {
        if self.sel == self.add_card() {
            self.name.extend(s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(31usize.saturating_sub(self.name.len())));
        }
    }

    pub fn backspace(&mut self) {
        self.name.pop();
    }

    /// Where the stars of card `i` are inside its picture (card-local pixels), fitted with a margin.
    fn star_points(&self, c: &Constellation, lay: &Layout) -> Vec<(f32, f32)> {
        let (x0, x1) = c.stars.iter().fold((f64::MAX, f64::MIN), |a, s| (a.0.min(s.1), a.1.max(s.1)));
        let (y0, y1) = c.stars.iter().fold((f64::MAX, f64::MIN), |a, s| (a.0.min(s.2), a.1.max(s.2)));
        let (bw, bh) = ((x1 - x0) as f32, (y1 - y0) as f32); // a single star has no width or height: it sits in the middle
        let (iw, ih) = (lay.card_w * 0.68, lay.card_h * 0.60);
        let s = (iw / bw.max(1.0)).min(ih / bh.max(1.0));
        let (ox, oy) = ((lay.card_w - bw * s) / 2.0, (lay.card_h - bh * s) / 2.0 - lay.card_h * 0.02);
        c.stars.iter().map(|st| (ox + (st.1 - x0) as f32 * s, oy + (st.2 - y0) as f32 * s)).collect()
    }

    /// One card's picture: a painted night, the figure, a moon in the corner.
    fn draw_card(&mut self, i: usize, lay: &Layout) -> Canvas {
        let p = palette();
        let (cw, ch) = (lay.card_w as usize, lay.card_h as usize);
        let mut cv = Canvas { w: cw, h: ch, px: vec![0; cw * ch] };
        let (fw, fh) = (lay.card_w, lay.card_h);
        let add = i == self.add_card();
        if !add { // the thin gold border: a plate the card sits on, so only its rim shows
            cv.round_rect(0.0, 0.0, fw, fh, 28.0, rgba(p.star, 0.5));
            cv.round_rect(1.4, 1.4, fw - 2.8, fh - 2.8, 26.6, rgba(p.dust, 1.0));
        } else {
            cv.round_rect(0.0, 0.0, fw, fh, 28.0, rgba(p.dust, 1.0));
        }
        // clouds along a diagonal, in the sky's own colors
        for k in 0..9u64 {
            let t = k as f32 / 8.0;
            let hv = hash(k + 31 * i as u64 + 5);
            let (x, y) = (fw * (0.05 + 0.9 * t), fh * (0.85 - 0.7 * t) + (unit(hv, 0) - 0.5) * fh * 0.14);
            glow(&mut cv, x, y, fh * (0.07 + unit(hv, 16) * 0.05), rgba(if k % 2 == 0 { p.mist } else { p.glow }, 0.55));
        }
        for k in 0..40u64 {
            let hv = hash(k + 77 + 13 * i as u64);
            cv.sparkle(unit(hv, 0) * fw, unit(hv, 16) * fh, 1.3 + unit(hv, 32) * 1.8, rgba(p.star, 0.25 + unit(hv, 48) * 0.45));
        }
        if add {
            let (cx, cy, arm) = (fw / 2.0, fh / 2.0, fh * 0.13);
            thread(&mut cv, (cx - arm, cy), (cx + arm, cy), 2, rgba(p.star, 0.8));
            thread(&mut cv, (cx, cy - arm), (cx, cy + arm), 5, rgba(p.star, 0.8));
            for (x, y) in [(cx - arm, cy), (cx + arm, cy), (cx, cy - arm), (cx, cy + arm)] {
                glow(&mut cv, x, y, 5.0, rgba(p.star, 0.7));
                cv.sparkle(x, y, 5.0, rgba(p.star, 1.0));
            }
        } else {
            let c = self.cards[i].clone();
            let pts = self.star_points(&c, lay);
            for (k, &(a, b)) in c.links.iter().enumerate() {
                thread(&mut cv, pts[a], pts[b], k, rgba(p.star, 0.9));
            }
            for (k, st) in c.stars.iter().enumerate() {
                let (x, y) = pts[k];
                let here = c.name == self.current && st.0 == self.current_star;
                glow(&mut cv, x, y, if here { 11.0 } else { 7.0 }, rgba(if here { p.ice } else { p.star }, 0.8));
                cv.sparkle(x, y, if here { 11.0 } else { 7.0 }, rgba(if here { p.ice } else { p.star }, 1.0));
                let label: String = st.0.chars().take(16).collect();
                let lw = self.t13.width(&label);
                self.t13.draw(&mut cv, x - lw / 2.0, y + 26.0, &label, rgba(p.text, if here { 0.9 } else { 0.5 }));
            }
            // a moon where you are: the constellation you are in
            let open = c.name == self.current;
            let (mx, my) = (fw - 52.0, fh - 52.0);
            if open {
                glow(&mut cv, mx, my, 10.0, rgba(p.text, 0.5));
            }
            cv.crescent(mx, my, 12.0, rgba(p.text, if open { 1.0 } else { 0.45 }));
        }
        // the border: a thin gold line, dashed for the add card
        if add {
            let (mut x, step) = (14.0, 22.0);
            while x < fw - 14.0 {
                let w = 12.0f32.min(fw - 14.0 - x);
                cv.round_rect(x, 0.0, w, 1.6, 0.8, rgba(p.star, 0.55));
                cv.round_rect(x, fh - 1.6, w, 1.6, 0.8, rgba(p.star, 0.55));
                x += step;
            }
            let mut y = 14.0;
            while y < fh - 14.0 {
                let h = 12.0f32.min(fh - 14.0 - y);
                cv.round_rect(0.0, y, 1.6, h, 0.8, rgba(p.star, 0.55));
                cv.round_rect(fw - 1.6, y, 1.6, h, 0.8, rgba(p.star, 0.55));
                y += step;
            }
        }
        clip_round(&mut cv, 28.0);
        cv
    }

    fn ensure_thumbs(&mut self, lay: &Layout) {
        let key = (lay.card_w as usize, lay.card_h as usize);
        if self.thumbs_for != key || self.thumbs.len() != self.len() {
            self.thumbs = (0..self.len()).map(|i| self.draw_card(i, lay)).collect();
            self.thumbs_for = key;
        }
    }

    /// A star on the middle card under a screen point, if any (only while the strip is at rest).
    fn star_at(&self, x: f32, y: f32, lay: &Layout) -> Option<usize> {
        if self.sliding() || self.sel >= self.cards.len() {
            return None;
        }
        let c = &self.cards[self.sel];
        let (ox, oy) = (lay.w / 2.0 - lay.card_w / 2.0, lay.cy - lay.card_h / 2.0);
        self.star_points(c, lay).iter().position(|&(sx, sy)| (ox + sx - x).hypot(oy + sy - y) <= STAR_REACH)
    }

    /// Arrow buttons: where they are.
    fn arrows(lay: &Layout) -> [(f32, f32); 2] {
        let off = lay.card_w / 2.0 + 44.0;
        [(lay.w / 2.0 - off, lay.cy), (lay.w / 2.0 + off, lay.cy)]
    }

    /// A click at a screen point.
    pub fn click(&mut self, x: f32, y: f32, w: f32, h: f32) -> Effect {
        let lay = Layout::new(w, h);
        let [l, r] = Self::arrows(&lay);
        if (x - l.0).hypot(y - l.1) < 22.0 {
            self.step(-1);
            return Effect::None;
        }
        if (x - r.0).hypot(y - r.1) < 22.0 {
            self.step(1);
            return Effect::None;
        }
        if let Some(star) = self.star_at(x, y, &lay) {
            return Effect::Open { name: self.cards[self.sel].name.clone(), star };
        }
        // a click on a side card brings it to the middle
        let pos = self.pos();
        for i in 0..self.len() {
            if i == self.sel {
                continue;
            }
            let (cx, scale, _) = lay.place(i as f32 - pos);
            if (x - cx).abs() < lay.card_w * scale / 2.0 && (y - lay.cy).abs() < lay.card_h * scale / 2.0 {
                self.select(i);
                break;
            }
        }
        Effect::None
    }

    /// Enter: open the card in the middle, or make the new constellation.
    pub fn activate(&mut self) -> Effect {
        if self.sel == self.add_card() {
            if self.name.is_empty() {
                self.msg = "type a name for it first..".into();
                return Effect::None;
            }
            return Effect::Create(self.name.clone());
        }
        let c = &self.cards[self.sel];
        let star = if c.name == self.current { c.stars.iter().position(|s| s.0 == self.current_star).unwrap_or(0) } else { 0 };
        Effect::Open { name: c.name.clone(), star }
    }

    /// One frame at visibility `e` (0 = gone, 1 = open).
    pub fn render(&mut self, w: usize, h: usize, e: f32) -> Canvas {
        let p = palette();
        let lay = Layout::new(w as f32, h as f32);
        self.ensure_thumbs(&lay);
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        // the dimmed sky, and the strip across the middle
        let a = 0.88 * e;
        let ch = |v: f32| (v * a * 255.0) as u32;
        cv.px.fill(((a * 255.0) as u32) << 24 | ch(p.sky[0]) << 16 | ch(p.sky[1]) << 8 | ch(p.sky[2]));
        let band = lay.band_bottom - lay.band_top;
        cv.round_rect(0.0, lay.band_top, lay.w, band, 0.0, rgba(p.sky, 0.85 * e));
        cv.round_rect(0.0, lay.band_top, lay.w, 1.2, 0.0, rgba(p.star, 0.3 * e));
        cv.round_rect(0.0, lay.band_bottom - 1.2, lay.w, 1.2, 0.0, rgba(p.star, 0.3 * e));
        let mut x = 6.0;
        while x < lay.w {
            cv.round_rect(x, lay.band_top + 10.0, 16.0, 8.0, 2.5, rgba(p.text, 0.45 * e)); // the film's perforations
            cv.round_rect(x, lay.band_bottom - 18.0, 16.0, 8.0, 2.5, rgba(p.text, 0.45 * e));
            x += 36.0;
        }

        let pos = self.pos();
        let mut order: Vec<usize> = (0..self.len()).collect();
        order.sort_by(|&a, &b| (b as f32 - pos).abs().total_cmp(&(a as f32 - pos).abs())); // far ones first
        for i in order {
            let d = i as f32 - pos;
            let (cx, scale, alpha) = lay.place(d);
            if alpha <= 0.01 {
                continue;
            }
            let s = scale * (0.94 + 0.06 * e);
            blit(&mut cv, &self.thumbs[i], cx, lay.cy, s, alpha * e);
            // the caption under the strip
            let caption = if i == self.add_card() { "add custom?".to_string() } else { format!("{}...", self.cards[i].name) };
            let near = 1.0 - d.abs().min(1.0);
            let t = if near > 0.5 { &mut self.t30 } else { &mut self.t18 };
            let tw = t.width(&caption);
            t.draw(&mut cv, cx - tw / 2.0, lay.band_bottom + lay.h * 0.065, &caption, rgba(p.text, (0.45 + 0.55 * near) * e * alpha.max(0.4)));
        }
        // the middle card's name field, when it is the add card
        if self.sel == self.add_card() && !self.sliding() {
            let shown = if self.name.is_empty() { "a name..".to_string() } else { self.name.clone() };
            let fw = lay.card_w * 0.55;
            let (fx, fy) = (lay.w / 2.0 - fw / 2.0, lay.cy + lay.card_h * 0.30);
            cv.round_rect(fx - 1.0, fy - 1.0, fw + 2.0, 40.0, 20.0, rgba(p.star, 0.5 * e));
            cv.round_rect(fx, fy, fw, 38.0, 19.0, rgba(p.sky, 0.9 * e));
            let end = self.t18.draw(&mut cv, fx + 20.0, fy + 25.0, &shown, rgba(p.text, if self.name.is_empty() { 0.35 } else { 1.0 } * e));
            if !self.name.is_empty() {
                cv.round_rect(end + 1.0, fy + 9.0, 1.5, 20.0, 0.0, rgba(p.text, 0.9 * e));
            }
        }
        // the arrows
        for (k, &(ax, ay)) in Self::arrows(&lay).iter().enumerate() {
            let on = if k == 0 { self.sel > 0 } else { self.sel + 1 < self.len() };
            let al = (if on { 0.8 } else { 0.25 }) * e;
            cv.round_rect(ax - 20.0, ay - 20.0, 40.0, 40.0, 20.0, rgba(p.text, 0.35 * al));
            cv.round_rect(ax - 19.0, ay - 19.0, 38.0, 38.0, 19.0, rgba(p.sky, 1.0 * e));
            let dir = if k == 0 { -1.0 } else { 1.0 };
            for s in [-1.0f32, 1.0] { // a chevron: two short strokes meeting at a tip
                for t in 0..10 {
                    let f = t as f32 / 9.0;
                    cv.round_rect(ax + dir * (3.0 - 7.0 * f) - 1.0, ay + s * 8.0 * f - 1.0, 2.0, 2.0, 1.0, rgba(p.text, al));
                }
            }
        }
        // a message, and what the keys do
        let hint = if self.sel == self.add_card() { "type a name  -  enter to make it  -  arrows to look around  -  esc to close" } else { "arrows to look around  -  enter or click a star to go there  -  esc to close" };
        let text = if self.msg.is_empty() { hint.to_string() } else { self.msg.clone() };
        let tw = self.t13.width(&text);
        self.t13.draw(&mut cv, lay.w / 2.0 - tw / 2.0, lay.h - 22.0, &text, rgba(p.text, 0.4 * e));
        cv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn con(name: &str, stars: &[(&str, f64, f64)], links: &[(usize, usize)]) -> Constellation {
        Constellation { name: name.into(), stars: stars.iter().map(|s| (s.0.to_string(), s.1, s.2)).collect(), links: links.to_vec() }
    }

    fn ui() -> Ui {
        Ui::new(
            vec![con("work", &[("a", 960.0, 540.0), ("b", 3360.0, 540.0), ("c", 3360.0, 2040.0)], &[(0, 1), (1, 2)]), con("chill", &[("x", 960.0, 540.0)], &[])],
            "work".into(),
            "b".into(),
        )
    }

    #[test]
    fn it_opens_on_the_constellation_you_are_in_and_cards_are_a_to_z() {
        let u = ui();
        assert_eq!(u.cards.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["chill", "work"]);
        assert_eq!(u.sel, 1);
        assert_eq!(u.len(), 3, "the add card is one more");
    }

    #[test]
    fn a_single_star_sits_in_the_middle_of_its_card() {
        let u = ui();
        let lay = Layout::new(1920.0, 1080.0);
        let pts = u.star_points(&u.cards[0].clone(), &lay); // "chill" has one star
        assert!((pts[0].0 - lay.card_w / 2.0).abs() < 1.0 && (pts[0].1 - lay.card_h / 2.0).abs() < lay.card_h * 0.05, "{pts:?}");
    }

    #[test]
    fn the_arrows_stop_at_the_ends() {
        let mut u = ui();
        u.step(5);
        assert_eq!(u.sel, 2);
        u.step(-9);
        assert_eq!(u.sel, 0);
    }

    #[test]
    fn enter_opens_the_star_you_are_in_or_the_first_of_another() {
        let mut u = ui();
        assert!(matches!(u.activate(), Effect::Open { ref name, star: 1 } if name == "work"), "you are in star b of work");
        u.select(0);
        assert!(matches!(u.activate(), Effect::Open { ref name, star: 0 } if name == "chill"));
    }

    #[test]
    fn the_add_card_needs_a_name_made_of_safe_characters() {
        let mut u = ui();
        u.select(2);
        assert!(matches!(u.activate(), Effect::None) && u.msg.contains("name"));
        u.type_text("my sky!");
        assert_eq!(u.name, "mysky");
        assert!(matches!(u.activate(), Effect::Create(ref n) if n == "mysky"));
        u.select(0);
        u.type_text("zzz"); // typing only goes to the add card
        assert_eq!(u.name, "mysky");
    }

    #[test]
    fn clicking_a_star_on_the_middle_card_opens_it_and_a_side_card_comes_forward() {
        let mut u = ui();
        u.slide = None;
        let (w, h) = (1920.0, 1080.0);
        let lay = Layout::new(w, h);
        let pts = u.star_points(&u.cards[1].clone(), &lay);
        let (ox, oy) = (w / 2.0 - lay.card_w / 2.0, lay.cy - lay.card_h / 2.0);
        assert!(matches!(u.click(ox + pts[2].0, oy + pts[2].1, w, h), Effect::Open { star: 2, .. }));
        // the card to the left is smaller and nearer the edge; clicking it selects it
        let (cx, scale, _) = lay.place(-1.0);
        assert!(scale < 1.0 && cx < w / 2.0);
        assert!(matches!(u.click(cx, lay.cy, w, h), Effect::None));
        assert_eq!(u.sel, 0);
    }

    #[test]
    fn it_draws_at_rest_mid_slide_and_faded_out_without_trouble() {
        let mut u = ui();
        let rest = u.render(1280, 720, 1.0);
        u.step(1);
        let sliding = u.render(1280, 720, 1.0);
        let gone = u.render(1280, 720, 0.0);
        assert_ne!(rest.px, sliding.px);
        assert!(gone.px.iter().all(|p| p >> 24 == 0), "fully faded out is fully clear");
    }
}
