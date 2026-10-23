//! The launcher's state and drawing, with no Wayland in it so `--dump` and the
//! tests can run it anywhere. Two looks share one state: a list, and stars.

use std::time::{Duration, Instant};

use seiza_common::{
    apps::{rank, App, Group, GROUPS},
    draw::{load_font, Canvas, Rgba, Text},
    palette::{palette, rgba},
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Style {
    List,
    Stars,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    App(Group),
    Command,
    Web,
}

pub struct Item {
    pub kind: Kind,
    pub label: String,
    pub line: String, // what gets sent to Seiza to start it
    pos: usize,       // place in the ranking, 0 = best match
}

pub const PANEL_W: f32 = 560.0;
const ROW_H: f32 = 56.0;
const HEAD_H: f32 = 76.0;
const FOOT_H: f32 = 44.0;
const MAX_ROWS: usize = 6;
const STARS_QUIET: usize = 6; // stars per cluster with no query
const STARS_BUSY: usize = 8;

pub struct Ui {
    pub style: Style,
    pub query: String,
    pub sel: usize,
    pub scroll: usize,
    pub w: f32,
    pub h: f32,
    pub error: Option<String>,
    apps: Vec<App>,
    items: Vec<Item>,
    t17: Text,
    t13: Text,
    content: Option<(String, Canvas)>, // the last drawn content and the state it was drawn for
    glide: Option<(f32, Instant)>,     // the list highlight sliding: the row it left, and when
    born: Instant,                     // the stars twinkle against this clock
}

const GLIDE: Duration = Duration::from_millis(140);

/// Draw `src` onto `dst` scaled by `s` about `c`, shifted down by `dy`, at opacity `a`.
/// `area` limits the work to the box the scaled content can reach (x, y, w, h in dst).
fn composite(dst: &mut Canvas, src: &Canvas, c: (f32, f32), s: f32, dy: f32, a: f32, area: Option<(f32, f32, f32, f32)>) {
    let (ax, ay, aw, ah) = area.unwrap_or((0.0, 0.0, dst.w as f32, dst.h as f32));
    let (x0, x1) = (ax.max(0.0) as usize, ((ax + aw) as usize).min(dst.w));
    let (y0, y1) = (ay.max(0.0) as usize, ((ay + ah) as usize).min(dst.h));
    let at = |x: i32, y: i32| -> [f32; 4] {
        if x < 0 || y < 0 || x >= src.w as i32 || y >= src.h as i32 {
            return [0.0; 4];
        }
        let p = src.px[y as usize * src.w + x as usize];
        [(p >> 24) as f32, (p >> 16 & 255) as f32, (p >> 8 & 255) as f32, (p & 255) as f32]
    };
    let still = s == 1.0 && dy.fract() == 0.0; // no resampling: most of the content is empty, skip it
    for y in y0..y1 {
        for x in x0..x1 {
            if still {
                let (sx, sy) = (x as i32, y as i32 - dy as i32);
                let p = if sx < 0 || sy < 0 || sx >= src.w as i32 || sy >= src.h as i32 { 0 } else { src.px[sy as usize * src.w + sx as usize] };
                if p == 0 {
                    continue;
                }
                let m = [(p >> 24) as f32, (p >> 16 & 255) as f32, (p >> 8 & 255) as f32, (p & 255) as f32];
                let sa = m[0] * a / 255.0;
                let d = dst.px[y * dst.w + x];
                let ch = |shift: u32, v: f32| ((d >> shift & 255) as f32 * (1.0 - sa) + v * a).min(255.0) as u32;
                dst.px[y * dst.w + x] = ((sa * 255.0).min(255.0) as u32 + ((d >> 24 & 255) as f32 * (1.0 - sa)) as u32).min(255) << 24 | ch(16, m[1]) << 16 | ch(8, m[2]) << 8 | ch(0, m[3]);
                continue;
            }
            let (sx, sy) = ((x as f32 + 0.5 - c.0) / s + c.0 - 0.5, (y as f32 + 0.5 - dy - c.1) / s + c.1 - 0.5);
            let (ix, iy, fx, fy) = (sx.floor() as i32, sy.floor() as i32, sx - sx.floor(), sy - sy.floor());
            let (p00, p10, p01, p11) = (at(ix, iy), at(ix + 1, iy), at(ix, iy + 1), at(ix + 1, iy + 1));
            let m = |i: usize| (p00[i] * (1.0 - fx) + p10[i] * fx) * (1.0 - fy) + (p01[i] * (1.0 - fx) + p11[i] * fx) * fy;
            let sa = m(0) * a / 255.0; // premultiplied: the source alpha scales every channel
            if sa <= 0.0 {
                continue;
            }
            let d = dst.px[y * dst.w + x];
            let ch = |shift: u32, v: f32| ((d >> shift & 255) as f32 * (1.0 - sa) + v * a).min(255.0) as u32;
            dst.px[y * dst.w + x] = ((sa * 255.0).min(255.0) as u32 + ((d >> 24 & 255) as f32 * (1.0 - sa)) as u32).min(255) << 24 | ch(16, m(1)) << 16 | ch(8, m(2)) << 8 | ch(0, m(3));
        }
    }
}

fn url_encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

/// Shorten `s` with ".." until it fits in `max` pixels.
fn fit(t: &mut Text, s: &str, max: f32) -> String {
    if t.width(s) <= max {
        return s.to_string();
    }
    let mut out: String = s.to_string();
    while !out.is_empty() && t.width(&format!("{out}..")) > max {
        out.pop();
    }
    format!("{out}..")
}

impl Ui {
    pub fn new(apps: Vec<App>, style: Style, w: f32, h: f32) -> Ui {
        let mut ui = Ui { style, query: String::new(), sel: 0, scroll: 0, w, h, error: None, apps, items: vec![], t17: Text::new(load_font(), 17.0), t13: Text::new(load_font(), 13.0), content: None, glide: None, born: Instant::now() };
        ui.refresh();
        ui
    }

    pub fn item(&self, i: usize) -> Option<&Item> {
        self.items.get(i)
    }

    /// Rebuild the items for the current query and select the best match.
    pub fn refresh(&mut self) {
        let q = self.query.trim().to_string();
        let ranked = rank(&self.apps, &q);
        self.items.clear();
        match self.style {
            Style::List => {
                for (pos, &i) in ranked.iter().enumerate() {
                    self.items.push(Item { kind: Kind::App(self.apps[i].group), label: self.apps[i].name.clone(), line: self.apps[i].exec.clone(), pos });
                }
                if !q.is_empty() {
                    self.items.push(Item { kind: Kind::Command, label: q.clone(), line: q.clone(), pos: usize::MAX });
                    self.items.push(Item { kind: Kind::Web, label: q.clone(), line: format!("xdg-open https://duckduckgo.com/?q={}", url_encode(&q)), pos: usize::MAX });
                }
            }
            Style::Stars => {
                let cap = if q.is_empty() { STARS_QUIET } else { STARS_BUSY };
                for (g, _) in GROUPS {
                    let mut n = 0;
                    for (pos, &i) in ranked.iter().enumerate() {
                        if self.apps[i].group == g && n < cap {
                            n += 1;
                            self.items.push(Item { kind: Kind::App(g), label: self.apps[i].name.clone(), line: self.apps[i].exec.clone(), pos });
                        }
                    }
                }
                if self.items.is_empty() && !q.is_empty() {
                    self.items.push(Item { kind: Kind::Command, label: q.clone(), line: q.clone(), pos: 0 });
                }
            }
        }
        self.sel = (0..self.items.len()).min_by_key(|&i| self.items[i].pos).unwrap_or(0);
        self.glide = None; // a new result list snaps
        self.scroll = 0;
        self.keep_visible();
    }

    pub fn toggle_style(&mut self) {
        self.style = if self.style == Style::List { Style::Stars } else { Style::List };
        self.refresh();
    }

    fn keep_visible(&mut self) {
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + MAX_ROWS {
            self.scroll = self.sel + 1 - MAX_ROWS;
        }
    }

    /// Select a row; in the list the highlight slides there.
    pub fn select(&mut self, i: usize) {
        if i != self.sel {
            self.glide = Some((self.highlight_row(), Instant::now()));
            self.sel = i;
        }
    }

    /// Where the list highlight is now, in rows (fractional while it slides).
    fn highlight_row(&self) -> f32 {
        let Some((from, t0)) = self.glide else { return self.sel as f32 };
        let t = (t0.elapsed().as_secs_f32() / GLIDE.as_secs_f32()).min(1.0);
        from + (self.sel as f32 - from) * (1.0 - (1.0 - t).powi(3))
    }

    /// True while something on screen is moving on its own: the highlight sliding.
    pub fn gliding(&self) -> bool {
        self.glide.is_some_and(|(_, t0)| t0.elapsed() < GLIDE)
    }

    /// Up/down in the list.
    pub fn step(&mut self, d: i32) {
        if !self.items.is_empty() {
            self.select((self.sel as i32 + d).clamp(0, self.items.len() as i32 - 1) as usize);
            self.keep_visible();
        }
    }

    pub fn scroll_by(&mut self, d: i32) {
        let max = self.items.len().saturating_sub(MAX_ROWS) as i32;
        self.scroll = (self.scroll as i32 + d).clamp(0, max) as usize;
    }

    /// Arrow keys in the stars: jump to the nearest star in that direction.
    pub fn drift(&mut self, dx: f32, dy: f32) {
        let pos = self.star_positions();
        let Some(&(sx, sy)) = pos.get(self.sel) else { return };
        let best = pos.iter().enumerate().filter(|&(i, _)| i != self.sel).filter_map(|(i, &(x, y))| {
            let (ax, ay) = ((x - sx) * dx + (y - sy) * dy, ((x - sx) * dy - (y - sy) * dx).abs()); // along, across
            (ax > 4.0).then_some((i, ax + 2.0 * ay))
        });
        if let Some((i, _)) = best.min_by(|a, b| a.1.total_cmp(&b.1)) {
            self.select(i);
        }
    }

    // ---- geometry ----

    fn panel(&self) -> (f32, f32, f32, f32) {
        let rows = self.items.len().clamp(1, MAX_ROWS) as f32;
        ((self.w - PANEL_W) / 2.0, self.h * 0.22, PANEL_W, HEAD_H + rows * ROW_H + FOOT_H)
    }

    /// Where each star sits: clusters in columns, a few slots per cluster, jittered by name.
    fn star_positions(&self) -> Vec<(f32, f32)> {
        const SLOTS: [(f32, f32); 8] = [(0.0, -1.0), (-0.9, 0.1), (0.9, 0.3), (-0.4, 1.0), (0.5, -0.2), (-0.1, 0.35), (1.0, -0.9), (-1.0, -0.7)];
        let col_w = self.w / GROUPS.len() as f32;
        let mut seen = [0usize; 4];
        self.items.iter().map(|it| {
            let g = match it.kind { Kind::App(g) => GROUPS.iter().position(|x| x.0 == g).unwrap(), _ => 1 };
            let slot = SLOTS[seen[g] % SLOTS.len()];
            seen[g] += 1;
            let j = hash(&it.label);
            let (jx, jy) = ((j % 21) as f32 - 10.0, (j / 21 % 21) as f32 - 10.0);
            (col_w * (g as f32 + 0.5) + slot.0 * col_w * 0.32 + jx, self.h * 0.60 + slot.1 * self.h * 0.15 + jy)
        }).collect()
    }

    /// The item under a point, if any.
    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        match self.style {
            Style::List => {
                let (px, py, pw, _) = self.panel();
                let (ry, shown) = (y - py - HEAD_H, self.items.len().min(MAX_ROWS));
                if x < px || x >= px + pw || ry < 0.0 || ry >= shown as f32 * ROW_H {
                    return None;
                }
                Some(self.scroll + (ry / ROW_H) as usize)
            }
            Style::Stars => self.star_positions().iter().position(|&(sx, sy)| (sx - x).hypot(sy - y) < 30.0),
        }
    }

    // ---- drawing ----

    fn quip(&self) -> String {
        match self.items.get(self.sel) {
            _ if self.query.trim().is_empty() => "where to..?".into(),
            Some(it) if matches!(it.kind, Kind::App(_)) => format!("{} again? ok..", it.label.to_lowercase()),
            Some(_) => "hm. let's try it..".into(),
            None => "nothing by that name..".into(),
        }
    }

    fn glow(cv: &mut Canvas, x: f32, y: f32, r: f32, c: Rgba) {
        for k in (0..14).rev() {
            let rr = r * (1.0 + k as f32 * 0.22);
            cv.round_rect(x - rr, y - rr, rr * 2.0, rr * 2.0, rr, [c[0], c[1], c[2], c[3] * 0.035]);
        }
    }

    fn put(&mut self, cv: &mut Canvas, x: f32, cy: f32, s: &str, big: bool, c: Rgba) -> f32 {
        let t = if big { &mut self.t17 } else { &mut self.t13 };
        let px = if big { 17.0 } else { 13.0 };
        t.draw(cv, x, cy + px * 0.33, s, c)
    }

    fn centered(&mut self, cv: &mut Canvas, cx: f32, cy: f32, s: &str, big: bool, c: Rgba) {
        let t = if big { &mut self.t17 } else { &mut self.t13 };
        let w = t.width(s);
        self.put(cv, cx - w / 2.0, cy, s, big, c);
    }

    /// One frame at visibility `e` (0 = gone, 1 = open). The content is drawn
    /// once per state change and only composited per frame, so animating is cheap.
    pub fn render(&mut self, e: f32) -> Canvas {
        let p = palette();
        // the key says when the content must be redrawn: any state change, a slide step, or an eighth of a second of twinkle
        let twinkle = if self.style == Style::Stars { (self.born.elapsed().as_secs_f32() * 8.0) as u64 } else { 0 };
        let key = format!("{:?}|{}|{}|{}|{:?}|{}x{}|{:.3}|{}", self.style, self.query, self.sel, self.scroll, self.error, self.w, self.h, self.highlight_row(), twinkle);
        if self.content.as_ref().is_none_or(|(k, _)| *k != key) {
            let (w, h) = (self.w as usize, self.h as usize);
            let mut cv = Canvas { w, h, px: vec![0; w * h] };
            match self.style {
                Style::List => self.render_list(&mut cv),
                Style::Stars => self.render_stars(&mut cv),
            }
            self.content = Some((key, cv));
        }
        let content = &self.content.as_ref().unwrap().1;
        let (w, h) = (content.w, content.h);
        let mut out = Canvas { w, h, px: vec![0; w * h] };
        let a = 0.86 * e; // a flat premultiplied fill: a rounded-rect pass over the whole screen is far slower
        let ch = |v: f32| (v * a * 255.0) as u32;
        out.px.fill(((a * 255.0) as u32) << 24 | ch(p.sky[0]) << 16 | ch(p.sky[1]) << 8 | ch(p.sky[2]));
        match self.style {
            Style::List => {
                let (px, py, pw, ph) = self.panel();
                let s = 0.9 + 0.1 * e; // the panel grows out of its own centre
                composite(&mut out, content, (px + pw / 2.0, py + ph / 2.0), s, 0.0, e, Some((px - 60.0, py - 60.0, pw + 120.0, ph + 120.0)));
            }
            Style::Stars => composite(&mut out, content, (self.w / 2.0, self.h / 2.0), 1.0, ((1.0 - e) * 18.0).round(), e, None), // drifts up into place
        }
        out
    }

    fn render_list(&mut self, cv: &mut Canvas) {
        let p = palette();
        let (px, py, pw, ph) = self.panel();
        cv.round_rect(px - 1.0, py - 1.0, pw + 2.0, ph + 2.0, 21.0, rgba(p.star, 0.35));
        cv.round_rect(px, py, pw, ph, 20.0, rgba(p.panel, 1.0));

        // search row: a ring with a sparkle, then the text pill
        let cy = py + HEAD_H / 2.0;
        cv.round_rect(px + 14.0, cy - 19.0, 38.0, 38.0, 19.0, rgba(p.star, 0.35));
        cv.round_rect(px + 15.0, cy - 18.0, 36.0, 36.0, 18.0, rgba(p.panel, 1.0));
        cv.sparkle(px + 33.0, cy, 8.0, rgba(p.star, 1.0));
        cv.round_rect(px + 62.0, cy - 19.0, pw - 76.0, 38.0, 19.0, rgba(p.sky, 0.55));
        self.input_text(cv, px + 78.0, cy, pw - 110.0);
        cv.round_rect(px, py + HEAD_H - 1.0, pw, 1.0, 0.0, rgba(p.text, 0.08));

        // rows
        let text = rgba(p.text, 1.0);
        let dim = rgba(p.text, 0.55);
        if self.items.is_empty() {
            self.put(cv, px + 24.0, py + HEAD_H + ROW_H / 2.0, "nothing by that name..", true, dim);
        }
        if !self.items.is_empty() {
            let hy = py + HEAD_H + (self.highlight_row() - self.scroll as f32) * ROW_H;
            cv.round_rect(px + 6.0, hy + 3.0, pw - 12.0, ROW_H - 6.0, 16.0, rgba(p.star, 0.4));
            cv.round_rect(px + 7.0, hy + 4.0, pw - 14.0, ROW_H - 8.0, 15.0, rgba(p.dust, 0.9));
        }
        for r in 0..self.items.len().saturating_sub(self.scroll).min(MAX_ROWS) {
            let i = self.scroll + r;
            let (ry, sel) = (py + HEAD_H + r as f32 * ROW_H, i == self.sel);
            let cy = ry + ROW_H / 2.0;
            cv.round_rect(px + 16.0, cy - 18.0, 36.0, 36.0, 12.0, rgba(p.sky, 0.8));
            let (kind, label) = (self.items[i].kind, self.items[i].label.clone());
            match kind {
                Kind::App(_) => cv.sparkle(px + 34.0, cy, 8.0, rgba(p.star, 1.0)),
                Kind::Command => cv.crescent(px + 34.0, cy, 9.0, rgba(p.text, 0.9)),
                Kind::Web => {
                    cv.round_rect(px + 25.0, cy - 9.0, 18.0, 18.0, 9.0, rgba(p.text, 0.8));
                    cv.round_rect(px + 26.5, cy - 7.5, 15.0, 15.0, 7.5, rgba(p.sky, 1.0));
                    cv.round_rect(px + 33.0, cy - 9.0, 2.0, 18.0, 1.0, rgba(p.text, 0.8));
                }
            }
            let max = pw - 150.0;
            match kind {
                Kind::App(_) => {
                    let s = fit(&mut self.t17, &label, max);
                    self.put(cv, px + 66.0, cy, &s, true, text);
                }
                _ => {
                    let s = fit(&mut self.t17, &label, max);
                    self.put(cv, px + 66.0, cy - 11.0, if kind == Kind::Web { "web search" } else { "command" }, false, dim);
                    self.put(cv, px + 66.0, cy + 8.0, &s, true, text);
                }
            }
            if sel {
                let tw = self.t13.width("open  enter");
                self.put(cv, px + pw - 22.0 - tw, cy, "open  enter", false, dim);
            }
        }

        // footer
        let fy = py + ph - FOOT_H / 2.0;
        cv.round_rect(px, py + ph - FOOT_H, pw, 1.0, 0.0, rgba(p.text, 0.08));
        let quip = self.error.clone().unwrap_or_else(|| self.quip());
        self.put(cv, px + 18.0, fy, &quip, false, rgba(p.text, 0.8));
        let hint = "up/down to drift  -  enter to open  -  tab: stars  -  esc to close";
        let hw = self.t13.width(hint);
        self.put(cv, px + pw - 18.0 - hw, fy, hint, false, rgba(p.text, 0.4));
    }

    /// The typed text (or the placeholder) and a caret, inside the search pill.
    fn input_text(&mut self, cv: &mut Canvas, x: f32, cy: f32, max: f32) {
        let p = palette();
        if self.query.is_empty() {
            self.put(cv, x, cy, "search the sky..", true, rgba(p.text, 0.35));
            cv.round_rect(x - 2.0, cy - 9.0, 1.5, 18.0, 0.0, rgba(p.text, 0.9));
            return;
        }
        let q = self.query.clone();
        // keep the tail visible when the text is longer than the pill
        let shown: String = q.chars().rev().scan(0.0, |w, c| { *w += self.t17.glyph(c).0.advance_width; (*w < max).then_some(c) }).collect::<Vec<_>>().into_iter().rev().collect();
        let end = self.put(cv, x, cy, &shown, true, rgba(p.text, 1.0));
        cv.round_rect(end + 1.0, cy - 9.0, 1.5, 18.0, 0.0, rgba(p.text, 0.9));
    }

    fn render_stars(&mut self, cv: &mut Canvas) {
        let p = palette();
        let pos = self.star_positions();

        // the search pill, with a quip under it
        let (pw, py) = (540.0, self.h * 0.12);
        let px = (self.w - pw) / 2.0;
        cv.round_rect(px - 1.0, py - 1.0, pw + 2.0, 50.0, 25.0, rgba(p.star, 0.35));
        cv.round_rect(px, py, pw, 48.0, 24.0, rgba(p.panel, 1.0));
        cv.sparkle(px + 28.0, py + 24.0, 8.0, rgba(p.star, 1.0));
        self.input_text(cv, px + 52.0, py + 24.0, pw - 170.0);
        let count = format!("{} {}", self.items.len(), if self.items.len() == 1 { "star" } else { "stars" });
        let cw = self.t13.width(&count);
        self.put(cv, px + pw - 24.0 - cw, py + 24.0, &count, false, rgba(p.text, 0.45));
        let quip = self.error.clone().unwrap_or_else(|| self.quip());
        self.centered(cv, self.w / 2.0, py + 76.0, &quip, true, rgba(p.text, 0.75));

        // threads between neighbours in a cluster, then the stars
        for i in 1..self.items.len() {
            if self.items[i].kind == self.items[i - 1].kind && matches!(self.items[i].kind, Kind::App(_)) {
                Self::thread(cv, pos[i - 1], pos[i], hash(&self.items[i].label), rgba(p.star, 0.6));
            }
        }
        for i in 0..self.items.len() {
            let (x, y) = pos[i];
            let sel = i == self.sel;
            let (r, a) = if sel { (11.0, 1.0) } else { (6.0, 0.75) };
            // slow, out-of-step twinkle: each star has its own period (about 6 to 11 s) and phase
            let h = hash(&self.items[i].label);
            let tw = ((self.born.elapsed().as_secs_f32() * (0.57 + (h % 50) as f32 / 100.0) + (h / 50 % 628) as f32 / 100.0).sin() + 1.0) / 2.0;
            let (r, a) = (r * (0.85 + 0.3 * tw), a * (0.8 + 0.2 * tw));
            Self::glow(cv, x, y, r, rgba(p.star, if sel { 0.9 } else { 0.5 }));
            cv.sparkle(x, y, r, rgba(if sel { p.ice } else { p.star }, a));
            if sel {
                cv.sparkle(x, y, r * 0.5, rgba(p.text, 1.0));
            }
            let label = self.items[i].label.to_lowercase();
            let s = fit(&mut self.t13, &label, 150.0);
            self.centered(cv, x, y + 30.0, &s, false, rgba(p.text, if sel { 1.0 } else { 0.55 }));
        }
        if self.items.is_empty() {
            self.centered(cv, self.w / 2.0, self.h * 0.55, "nothing here yet..", true, rgba(p.text, 0.4));
        }

        // cluster names and the hint
        let col_w = self.w / GROUPS.len() as f32;
        for (g, (_, name)) in GROUPS.iter().enumerate() {
            self.centered(cv, col_w * (g as f32 + 0.5), self.h * 0.88, name, true, rgba(p.text, 0.5));
        }
        self.centered(cv, self.w / 2.0, self.h - 28.0, "arrows to drift  -  enter to open  -  tab: list  -  esc to close", false, rgba(p.text, 0.4));
    }

    /// A hand-inked arc between two stars: doubled, a little wobbly.
    fn thread(cv: &mut Canvas, a: (f32, f32), b: (f32, f32), seed: u64, c: Rgba) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = dx.hypot(dy).max(1.0);
        let bend = len * (0.12 + (seed % 10) as f32 * 0.01) * if seed % 2 == 0 { 1.0 } else { -1.0 };
        let ctrl = ((a.0 + b.0) / 2.0 - dy / len * bend, (a.1 + b.1) / 2.0 + dx / len * bend);
        for pass in 0..2 {
            let steps = (len / 2.5) as i32;
            for s in 0..=steps {
                let t = s as f32 / steps as f32;
                let (u, v) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t));
                let wob = if pass == 0 { 0.0 } else { 1.6 * (t * 9.0 + seed as f32).sin() + 1.2 };
                let x = u * a.0 + v * ctrl.0 + t * t * b.0 - dy / len * wob;
                let y = u * a.1 + v * ctrl.1 + t * t * b.1 + dx / len * wob;
                cv.round_rect(x - 0.7, y - 0.7, 1.4, 1.4, 0.7, [c[0], c[1], c[2], c[3] * if pass == 0 { 1.0 } else { 0.4 }]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, group: Group) -> App {
        App { name: name.into(), exec: format!("run-{name}"), extra: String::new(), group }
    }

    fn ui(style: Style) -> Ui {
        Ui::new(vec![app("Files", Group::System), app("Firefox", Group::Web), app("Krita", Group::Make), app("Terminal", Group::Make)], style, 1920.0, 1080.0)
    }

    #[test]
    fn list_adds_command_and_web_rows_for_a_query() {
        let mut u = ui(Style::List);
        assert_eq!(u.items.len(), 4);
        u.query = "fir e".into();
        u.refresh();
        assert!(u.items.iter().all(|i| matches!(i.kind, Kind::Command | Kind::Web)), "no app matches 'fir e'");
        assert_eq!(u.items[1].line, "xdg-open https://duckduckgo.com/?q=fir%20e");
        u.query = "fire".into();
        u.refresh();
        assert_eq!((u.items[0].label.as_str(), u.sel), ("Firefox", 0));
        assert_eq!(u.items.len(), 3);
    }

    #[test]
    fn stars_select_the_best_match_and_drift_by_direction() {
        let mut u = ui(Style::Stars);
        u.query = "term".into();
        u.refresh();
        assert_eq!(u.items[u.sel].label, "Terminal");
        u.query.clear();
        u.refresh();
        let start = u.sel;
        for d in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            u.drift(d.0, d.1);
        }
        assert!(u.sel < u.items.len());
        let _ = start;
    }

    #[test]
    fn a_click_lands_on_the_row_it_is_over() {
        let u = ui(Style::List);
        let (px, py, _, _) = u.panel();
        assert_eq!(u.hit(px + 20.0, py + HEAD_H + ROW_H * 2.5), Some(2));
        assert_eq!(u.hit(px - 5.0, py + HEAD_H + 5.0), None);
        let s = ui(Style::Stars);
        let (x, y) = s.star_positions()[1];
        assert_eq!(s.hit(x + 3.0, y - 3.0), Some(1));
    }
}
