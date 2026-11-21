//! The look the help, the settings and the panel share: cards, keycaps and the thread of stars that
//! switches pages. Colours come from the palette, so a new wallpaper recolours all of it.

use std::time::Instant;

use crate::draw::{Canvas, Text};
use crate::palette::{palette, rgba};

pub const CARD_R: f32 = 14.0;
/// Height of a card's header strip (its title and the hairline under it).
pub const CARD_HEAD: f32 = 42.0;

/// A card at (x, y): a plate a little lighter than the window with a hairline edge and a small title.
/// Draw its rows below `CARD_HEAD`.
pub fn card(cv: &mut Canvas, title_font: &mut Text, x: f32, y: f32, w: f32, h: f32, title: &str) {
    let p = palette();
    cv.round_rect(x, y, w, h, CARD_R, rgba(p.text, 0.10));
    cv.round_rect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, CARD_R - 1.0, rgba(p.panel, 1.0));
    cv.round_rect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, CARD_R - 1.0, rgba(p.text, 0.035));
    title_font.draw(cv, x + 18.0, y + 26.0, title, rgba(p.text, 0.5));
    cv.round_rect(x + 1.0, y + CARD_HEAD, w - 2.0, 1.0, 0.0, rgba(p.text, 0.07));
}

/// How wide `keycap` draws `label`.
pub fn keycap_w(font: &mut Text, label: &str) -> f32 {
    font.width(label) + 20.0
}

/// One keycap (24 tall) at (x, y); returns where the next thing starts.
pub fn keycap(cv: &mut Canvas, font: &mut Text, x: f32, y: f32, label: &str) -> f32 {
    let p = palette();
    let w = keycap_w(font, label);
    cv.round_rect(x, y, w, 24.0, 7.0, rgba(p.text, 0.32));
    cv.round_rect(x + 1.0, y + 1.0, w - 2.0, 22.0, 6.0, rgba(p.sky, 1.0));
    font.draw(cv, x + 10.0, y + 17.0, label, rgba(p.text, 0.92));
    x + w
}

/// How far each star sits above (-) or below (+) the thread's middle, so the row reads as a loose wave.
const WAVE: [f32; 8] = [-1.0, 7.0, -5.0, 5.0, -8.0, 2.0, -3.0, 6.0];

pub const OPEN_SECS: f32 = 0.30;
pub const CLOSE_SECS: f32 = 0.18;
/// How far below its place a card starts, and ends when it closes.
pub const SLIDE: f32 = 90.0;
pub const GLIDE_SECS: f32 = 0.16;
/// Width of the search pill.
pub const PILL_W: f32 = 250.0;

/// When a window opened and when it began closing; says how visible it is.
#[derive(Default)]
pub struct Anim {
    opened: Option<Instant>,
    closing: Option<Instant>,
}

impl Anim {
    pub fn open(&mut self) {
        self.opened.get_or_insert_with(Instant::now);
    }

    pub fn close(&mut self) {
        self.closing.get_or_insert_with(Instant::now);
    }

    pub fn is_closing(&self) -> bool {
        self.closing.is_some()
    }

    /// (visibility 0..1, whether it is still moving, whether it has finished closing).
    pub fn visibility(&self) -> (f32, bool, bool) {
        let t = self.opened.map_or(0.0, |o| (o.elapsed().as_secs_f32() / OPEN_SECS).min(1.0));
        let open = 1.0 - (1.0 - t).powi(3); // eases out: fast, then settles
        let Some(c) = self.closing else { return (open, t < 1.0, false) };
        let ct = (c.elapsed().as_secs_f32() / CLOSE_SECS).min(1.0);
        (open.min((1.0 - ct).powi(2)), true, ct >= 1.0)
    }
}

/// A star (or any marker) moving to a new page.
#[derive(Default)]
pub struct Glide(Option<(f32, Instant)>);

impl Glide {
    /// Begin moving from page `from`.
    pub fn start(&mut self, from: f32) {
        self.0 = Some((from, Instant::now()));
    }

    /// Where it is on its way to page `to`.
    pub fn pos(&self, to: usize) -> f32 {
        let Some((from, t)) = self.0 else { return to as f32 };
        let k = (t.elapsed().as_secs_f32() / GLIDE_SECS).min(1.0);
        from + (to as f32 - from) * (1.0 - (1.0 - k).powi(3)) // eases out
    }

    pub fn active(&self) -> bool {
        self.0.is_some_and(|(_, t)| t.elapsed().as_secs_f32() < GLIDE_SECS)
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }
}

/// `content` at visibility `e`: lower and fainter the less visible it is. Only `rows` of it are read.
pub fn appear(content: &Canvas, e: f32, rows: std::ops::Range<usize>) -> Canvas {
    if e >= 1.0 {
        return content.clone();
    }
    let mut out = Canvas { w: content.w, h: content.h, px: vec![0; content.w * content.h] };
    out.over_faded(content, ((1.0 - e) * SLIDE).round() as i32, e, rows);
    out
}

/// A window's plate on a `w` x `h` canvas: a gold glow and edge round the card at (ox, oy), its fill, and a
/// hairline `foot` from the bottom for the footer text.
pub fn window_plate(w: usize, h: usize, (ox, oy): (f32, f32), (cw, ch): (f32, f32), foot: f32) -> Canvas {
    let p = palette();
    let mut b = Canvas { w, h, px: vec![0; w * h] };
    for k in (1..=3).rev() {
        let k = k as f32 * 4.0;
        b.round_rect(ox - 1.0 - k, oy - 1.0 - k, cw + 2.0 + 2.0 * k, ch + 2.0 + 2.0 * k, 25.0 + k, rgba(p.star, 0.035));
    }
    b.round_rect(ox - 1.0, oy - 1.0, cw + 2.0, ch + 2.0, 25.0, rgba(p.star, 0.5));
    b.round_rect(ox, oy, cw, ch, 24.0, rgba(p.panel, 1.0));
    b.round_rect(ox + 1.0, oy + ch - foot + 8.0, cw - 2.0, 1.0, 0.0, rgba(p.text, 0.07));
    b
}

/// The search pill at (x, y): `hint` while empty, else the end of what is typed and a caret.
pub fn search_pill(cv: &mut Canvas, font: &mut Text, x: f32, y: f32, query: &str, hint: &str) {
    let p = palette();
    cv.round_rect(x, y, PILL_W, 34.0, 17.0, rgba(p.text, 0.12));
    cv.round_rect(x + 1.0, y + 1.0, PILL_W - 2.0, 32.0, 16.0, rgba(p.sky, 1.0));
    if query.is_empty() {
        font.draw(cv, x + 18.0, y + 22.0, hint, rgba(p.text, 0.4));
        return;
    }
    let mut shown = query; // too long to fit: its end, where the typing is
    while font.width(shown) > PILL_W - 44.0 {
        shown = &shown[shown.chars().next().map_or(0, char::len_utf8)..];
    }
    let end = font.draw(cv, x + 18.0, y + 22.0, shown, rgba(p.text, 0.95));
    cv.round_rect(end + 1.0, y + 9.0, 1.5, 16.0, 0.0, rgba(p.star, 0.9));
}

/// A row of pages drawn as stars `pos` is the (fractional) page the big star is at, so
/// it can glide; stars grow and glow as it nears them.
pub struct Thread {
    chain: Vec<(f32, f32)>, // the thread's loose start, the stars, its loose end
    n: usize,
}

impl Thread {
    /// `n` stars spread between x0 and x1 around the height `y`.
    pub fn new(x0: f32, x1: f32, y: f32, n: usize) -> Thread {
        let m = 52.0_f32.min((x1 - x0) / 4.0);
        let step = if n > 1 { (x1 - x0 - 2.0 * m) / (n - 1) as f32 } else { 0.0 };
        let stars: Vec<(f32, f32)> = (0..n).map(|i| (x0 + m + i as f32 * step, y + WAVE[i % WAVE.len()])).collect();
        let (first, last) = (stars[0], stars[n - 1]);
        let mut chain = vec![(x0, first.1 + 7.0)];
        chain.extend(stars);
        chain.push((x1, last.1 - 9.0));
        Thread { chain, n }
    }

    pub fn star(&self, i: usize) -> (f32, f32) {
        self.chain[i + 1]
    }

    /// A point on the thread: `u` counts stars (0 is the first, 1 the second...), -1 is the loose start.
    fn at(&self, u: f32) -> (f32, f32) {
        let j = (u.floor() + 1.0).clamp(0.0, (self.chain.len() - 2) as f32) as usize;
        let t = (u + 1.0 - j as f32).clamp(0.0, 1.0);
        let c = |k: isize| self.chain[k.clamp(0, self.chain.len() as isize - 1) as usize];
        let (p0, p1, p2, p3) = (c(j as isize - 1), c(j as isize), c(j as isize + 1), c(j as isize + 2));
        let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t + (-a + 3.0 * b - 3.0 * c + d) * t * t * t); // Catmull-Rom
        (f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1))
    }

    /// The page whose star or name is at (x, y).
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        let half = if self.n > 1 { (self.chain[2].0 - self.chain[1].0) / 2.0 } else { 40.0 };
        (0..self.n).find(|&i| {
            let (sx, sy) = self.star(i);
            (x - sx).abs() < half && y > sy - 22.0 && y < sy + 42.0
        })
    }

    pub fn draw(&self, cv: &mut Canvas, font: &mut Text, labels: &[&str], pos: f32, hover: Option<usize>) {
        let p = palette();
        let line: Vec<(f32, f32)> = (0..=(self.n + 1) * 10).map(|k| self.at(-1.0 + k as f32 / 10.0)).collect();
        cv.polyline(&line, 1.3, rgba(p.star, 0.5));
        for i in 0..self.n {
            let (x, y) = self.star(i);
            let near = (1.0 - (i as f32 - pos).abs()).max(0.0); // 1 on the big star, 0 a page away
            let lift = if hover == Some(i) { 0.25 } else { 0.0 };
            let r = 6.0 + 7.5 * near + 2.0 * lift;
            if near > 0.0 {
                cv.round_rect(x - r * 2.2, y - r * 2.2, r * 4.4, r * 4.4, r * 2.2, rgba(p.star, 0.05 * near));
                cv.round_rect(x - r * 1.5, y - r * 1.5, r * 3.0, r * 3.0, r * 1.5, rgba(p.star, 0.08 * near));
            }
            cv.sparkle(x, y, r, rgba(p.star, (0.7 + 0.3 * near + lift).min(1.0)));
            let w = font.width(labels[i]);
            font.draw(cv, x - w / 2.0, y + 26.0, labels[i], rgba(p.text, 0.5 + 0.45 * near + 0.25 * lift));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_thread_passes_through_its_stars_and_finds_them_again() {
        let t = Thread::new(100.0, 900.0, 50.0, 6);
        for i in 0..6 {
            let (sx, sy) = t.star(i);
            let (ax, ay) = t.at(i as f32);
            assert!((sx - ax).abs() < 0.01 && (sy - ay).abs() < 0.01, "star {i}");
            assert_eq!(t.hit(sx, sy + 20.0), Some(i));
        }
        assert_eq!(t.hit(500.0, 400.0), None);
        let (mx, _) = t.at(2.5);
        assert!(mx > t.star(2).0 && mx < t.star(3).0, "halfway is between the two stars");
    }

    #[test]
    fn a_window_that_closes_ends_at_zero_and_says_so() {
        let mut a = Anim::default();
        assert_eq!(a.visibility().0, 0.0, "not open yet");
        a.open();
        std::thread::sleep(std::time::Duration::from_millis((OPEN_SECS * 1000.0) as u64 + 30));
        assert_eq!(a.visibility(), (1.0, false, false));
        a.close();
        assert!(a.is_closing() && a.visibility().1);
        std::thread::sleep(std::time::Duration::from_millis((CLOSE_SECS * 1000.0) as u64 + 30));
        assert_eq!(a.visibility(), (0.0, true, true));
    }

    #[test]
    fn a_glide_lands_on_its_page() {
        let mut g = Glide::default();
        assert_eq!(g.pos(3), 3.0);
        g.start(0.0);
        assert!(g.active() && g.pos(3) < 1.0);
        std::thread::sleep(std::time::Duration::from_millis((GLIDE_SECS * 1000.0) as u64 + 30));
        assert!(!g.active());
        assert_eq!(g.pos(3), 3.0);
    }

    #[test]
    fn over_puts_source_over_destination() {
        let mut a = Canvas { w: 2, h: 1, px: vec![0xFF00_0000, 0xFF00_0000] };
        let b = Canvas { w: 1, h: 1, px: vec![0x8040_0000] }; // half-covering premultiplied red
        a.over(&b, 1, 0);
        assert_eq!(a.px[0], 0xFF00_0000);
        assert_eq!(a.px[1] >> 24, 255);
        assert!(a.px[1] >> 16 & 255 >= 0x40);
    }
}
