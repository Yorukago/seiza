//! The widgets' state and drawing: a clock with the moon's phase, a calendar and
//! the machine's pulse, as cards in a row. No Wayland here, so `--dump` can render it.

use crate::feeds::Weather;
use std::collections::HashMap;

use seiza_common::{
    draw::{shared_font, Canvas, Text},
    local_time,
    palette::{palette, rgba},
};

const GAP: f32 = 22.0;
const TOP: f32 = 64.0; // where the first card sits until it is dragged: under the bar
const MARGIN: f32 = 26.0; // from the left edge
/// Where cards sit until they are dragged: two columns on the left and middle of the sky, so the top-right
/// stays free for the toasts. Each column lists its cards top to bottom.
const COLUMNS: [&[&str]; 2] = [&["clock", "calendar"], &["system", "media", "weather", "agenda"]];
const HISTORY: usize = 60;
const MEDIA_BTN_Y: f32 = 146.0;
const BUTTONS: [(f32, &str); 3] = [(-44.0, "previous"), (0.0, "play-pause"), (44.0, "next")];
const CARDS: [&str; 6] = ["clock", "calendar", "system", "media", "weather", "agenda"];

/// What the system card shows; filled by the main loop once a second.
#[derive(Default, Clone, PartialEq)]
pub struct Pulse {
    pub cpu: Option<u32>,
    pub ram: Option<u32>,
    pub temp: Option<u32>,
    pub load: Option<f32>,
}

/// What the media card shows: the first playing MPRIS player, else the first one there is.
#[derive(Default, Clone, PartialEq)]
pub struct Media {
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub pos: u64, // seconds
    pub len: u64,
    pub art: String, // path of a local cover picture, else empty
}

/// One line per player from `playerctl -a metadata --format <FORMAT>`.
pub const MEDIA_FORMAT: &str = "{{status}}\u{1f}{{title}}\u{1f}{{artist}}\u{1f}{{position}}\u{1f}{{mpris:length}}\u{1f}{{mpris:artUrl}}";

pub fn parse_media(out: &str) -> Option<Media> {
    let players: Vec<Media> = out
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\u{1f}').collect();
            let secs = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) / 1_000_000;
            (f.len() == 6 && f[0] != "Stopped").then(|| Media { playing: f[0] == "Playing", title: f[1].into(), artist: f[2].into(), pos: secs(3), len: secs(4), art: f[5].strip_prefix("file://").unwrap_or("").replace("%20", " ") })
        })
        .collect();
    players.iter().find(|m| m.playing).or(players.first()).cloned()
}

/// Where a card lives: `dx`,`dy` from the center of one star, in world units, so it is there
/// when that star is in view and out of sight when it is not.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub constellation: String,
    pub star: String,
    pub dx: f32,
    pub dy: f32,
}

/// What the compositor says of the camera. A world point is on screen at `world * zoom - cam`.
#[derive(Clone, Default)]
pub struct View {
    pub constellation: String,
    pub current: String,
    pub stars: Vec<(String, f32, f32)>, // name, center x, center y
    pub zoom: f32,
    pub cam: (f32, f32),
}

struct Drag {
    name: String,
    grab: (f32, f32), // where in the card the pointer took hold
    at: (f32, f32),   // top-left on screen now
    from: (f32, f32), // top-left when it was picked up
}

pub struct Ui {
    pub cards: Vec<String>, // "clock", "calendar", "system", "media", in the order drawn
    pub pulse: Pulse,
    pub media: Option<Media>,
    pub weather: Option<Weather>,
    pub agenda: Option<Vec<(String, String)>>, // (when, what); None = no calendar set
    art: (String, Option<Canvas>), // the picture for the path last asked for
    pub pin: HashMap<String, Pin>, // cards the user has dragged: each lives on one star
    pub view: Option<View>,        // the camera and the stars, from the compositor's state stream
    drag: Option<Drag>,
    cache_key: String,
    cache: HashMap<String, Canvas>, // each card drawn once per change, so a drag only copies pixels
    cpu_history: Vec<u32>,
    t64: Text,
    t18: Text,
    t14: Text,
}

const CARD_PAD: usize = 2;

/// `src` over `dst` with its top-left at (ox, oy); both are premultiplied ARGB.
fn blit(dst: &mut Canvas, src: &Canvas, ox: i32, oy: i32) {
    for sy in 0..src.h {
        let dy = oy + sy as i32;
        if dy < 0 || dy as usize >= dst.h {
            continue;
        }
        for sx in 0..src.w {
            let dx = ox + sx as i32;
            let s = src.px[sy * src.w + sx];
            if s == 0 || dx < 0 || dx as usize >= dst.w {
                continue;
            }
            let d = &mut dst.px[dy as usize * dst.w + dx as usize];
            let inv = 255 - (s >> 24);
            let ch = |shift: u32| ((s >> shift & 0xFF) + (*d >> shift & 0xFF) * inv / 255).min(255);
            *d = ch(24) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0);
        }
    }
}

/// `src` over `dst` at `scale` (premultiplied ARGB): averaged over the source pixels each destination pixel covers
/// when smaller, bilinear when bigger. Its top-left lands on (ox, oy).
fn blit_scaled(dst: &mut Canvas, src: &Canvas, ox: i32, oy: i32, scale: f32) {
    if (scale - 1.0).abs() < 1e-3 {
        return blit(dst, src, ox, oy);
    }
    let (tw, th) = ((src.w as f32 * scale).ceil() as i32, (src.h as f32 * scale).ceil() as i32);
    let at = |x: i32, y: i32| src.px[y.clamp(0, src.h as i32 - 1) as usize * src.w + x.clamp(0, src.w as i32 - 1) as usize];
    for ty in 0..th {
        let dy = oy + ty;
        if dy < 0 || dy as usize >= dst.h {
            continue;
        }
        for tx in 0..tw {
            let dx = ox + tx;
            if dx < 0 || dx as usize >= dst.w {
                continue;
            }
            let mut sum = [0.0f32; 4];
            let mut put = |px: u32, wt: f32| (0..4).for_each(|c| sum[c] += (px >> (8 * c) & 0xFF) as f32 * wt);
            if scale < 1.0 {
                let (x0, x1, y0, y1) = (tx as f32 / scale, (tx + 1) as f32 / scale, ty as f32 / scale, (ty + 1) as f32 / scale);
                let area = (x1.min(src.w as f32) - x0) * (y1.min(src.h as f32) - y0);
                if area <= 0.0 {
                    continue;
                }
                for sy in y0 as i32..y1.ceil() as i32 {
                    for sx in x0 as i32..x1.ceil() as i32 {
                        let wx = (x1.min(sx as f32 + 1.0) - x0.max(sx as f32)).max(0.0);
                        let wy = (y1.min(sy as f32 + 1.0) - y0.max(sy as f32)).max(0.0);
                        if sx < src.w as i32 && sy < src.h as i32 {
                            put(at(sx, sy), wx * wy / area);
                        }
                    }
                }
            } else {
                let (fx, fy) = ((tx as f32 + 0.5) / scale - 0.5, (ty as f32 + 0.5) / scale - 0.5);
                let (ix, iy) = (fx.floor(), fy.floor());
                let (ax, ay) = (fx - ix, fy - iy);
                for (ddx, ddy, wt) in [(0, 0, (1.0 - ax) * (1.0 - ay)), (1, 0, ax * (1.0 - ay)), (0, 1, (1.0 - ax) * ay), (1, 1, ax * ay)] {
                    put(at(ix as i32 + ddx, iy as i32 + ddy), wt);
                }
            }
            let [b, g, r, a] = sum.map(|v| v.round() as u32);
            let s = a << 24 | r << 16 | g << 8 | b;
            if s == 0 {
                continue;
            }
            let d = &mut dst.px[dy as usize * dst.w + dx as usize];
            let inv = 255 - a;
            let ch = |shift: u32| ((s >> shift & 0xFF) + (*d >> shift & 0xFF) * inv / 255).min(255);
            *d = ch(24) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0);
        }
    }
}

/// `s` cut to fit `max` pixels, with ".." when it was cut.
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

/// 83 -> "1:23"
const ART: usize = 64;

/// The picture at `path` as a square of ART px with rounded corners; None without ImageMagick or if it can't be read.
fn cover(path: &str) -> Option<Canvas> {
    let sz = format!("{ART}x{ART}");
    let out = std::process::Command::new("magick").args([&format!("{path}[0]"), "-resize", &format!("{sz}^"), "-gravity", "center", "-extent", &sz, "ppm:-"]).output().ok()?;
    let nl = out.stdout.iter().enumerate().filter(|(_, b)| **b == b'\n').nth(2)?.0 + 1;
    let rgb = out.stdout.get(nl..nl + ART * ART * 3)?;
    let r = 10.0f32;
    let px = rgb.chunks(3).enumerate().map(|(i, c)| {
        let (x, y) = ((i % ART) as f32 + 0.5, (i / ART) as f32 + 0.5);
        let (dx, dy) = ((r - x).max(x - (ART as f32 - r)).max(0.0), (r - y).max(y - (ART as f32 - r)).max(0.0));
        if dx * dx + dy * dy > r * r { 0 } else { 0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32 }
    });
    Some(Canvas { w: ART, h: ART, px: px.collect() })
}

fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
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
        Ui { cards, pulse: Pulse::default(), media: None, weather: None, agenda: None, art: Default::default(), pin: HashMap::new(), view: None, drag: None, cache_key: String::new(), cache: HashMap::new(), cpu_history: vec![], t64: Text::new(shared_font(), 64.0), t18: Text::new(shared_font(), 18.0), t14: Text::new(shared_font(), 14.0) }
    }

    pub fn set_pulse(&mut self, p: Pulse) {
        self.cpu_history.push(p.cpu.unwrap_or(0));
        if self.cpu_history.len() > HISTORY {
            self.cpu_history.remove(0);
        }
        self.pulse = p;
    }

    /// Redraw when this changes.
    /// Take the player's state; the cover is decoded once per picture (ImageMagick, like settings thumbnails).
    pub fn set_media(&mut self, m: Option<Media>) {
        let path = m.as_ref().map(|m| m.art.clone()).unwrap_or_default();
        if path != self.art.0 {
            self.art = (path.clone(), if path.is_empty() { None } else { cover(&path) });
        }
        self.media = m;
    }

    pub fn key(&self) -> String {
        let m = self.media.as_ref().map(|m| format!("{}{}{}{}{}", m.playing, m.title, m.artist, m.pos, m.art));
        format!("{}|{}|{}|{m:?}|{:?}|{:?}", local_time(c"%Y%m%d%H%M"), self.cpu_history.len(), self.cpu_history.last().copied().unwrap_or(0), self.weather, self.agenda)
    }

    fn card_size(name: &str) -> (f32, f32) {
        match name {
            "clock" => (340.0, 150.0),
            "calendar" => (340.0, 300.0),
            "media" => (340.0, 170.0),
            "weather" => (340.0, 150.0),
            "agenda" => (340.0, 200.0),
            _ => (340.0, 236.0),
        }
    }

    fn visible(&self) -> Vec<String> {
        self.cards.iter().filter(|c| CARDS.contains(&c.as_str())).cloned().collect()
    }

    pub fn width(&self) -> u32 {
        (self.visible().iter().map(|c| Self::card_size(c).0 + GAP).sum::<f32>() - GAP).max(1.0) as u32
    }

    pub fn set_view(&mut self, st: &seiza_common::State) {
        let stars = st.stars.iter().map(|s| (s.name.clone(), s.x as f32, s.y as f32)).collect();
        self.view = Some(View { constellation: st.constellation.clone(), current: st.current.clone(), stars, zoom: st.zoom as f32, cam: (st.cam.0 as f32, st.cam.1 as f32) });
    }

    /// Cards shrink and grow with the camera's zoom, like the windows do.
    fn scale(&self) -> f32 {
        self.view.as_ref().map_or(1.0, |v| v.zoom)
    }

    /// A card's place on a `w` x `h` screen: (x, y, width, height), or None while it is not in view.
    /// Every card lives on a star. Ones never dragged sit in a centered row on the first star; held ones stay on the screen.
    pub fn rect(&self, name: &str, w: f32, h: f32) -> Option<(f32, f32, f32, f32)> {
        let (nw, nh) = Self::card_size(name);
        let z = self.scale();
        let (cw, ch) = (nw * z, nh * z);
        let on_screen = |x: f32, y: f32| (x.clamp(0.0, (w - cw).max(0.0)), y.clamp(0.0, (h - ch).max(0.0)), cw, ch);
        if let Some(d) = self.drag.as_ref().filter(|d| d.name == name) {
            return Some(on_screen(d.at.0, d.at.1));
        }
        let Some(pin) = self.pin.get(name) else {
            let (hx, hy) = self.home(name);
            let (x, y, ..) = on_screen(hx, hy);
            let Some(v) = &self.view else { return Some((x, y, cw, ch)) }; // before the first state: where it will be
            let &(_, sx, sy) = v.stars.first()?;
            // the spot that is (x, y) when the first star is centered, as an offset from that star
            let (cx, cy) = (sx + x - w / 2.0, sy + y - h / 2.0);
            return Self::in_view(v, (cx, cy), cw, ch, w, h);
        };
        let v = self.view.as_ref().filter(|v| v.constellation == pin.constellation)?;
        let &(_, sx, sy) = v.stars.iter().find(|(n, _, _)| *n == pin.star)?;
        Self::in_view(v, (sx + pin.dx, sy + pin.dy), cw, ch, w, h)
    }

    /// Where a card that was never dragged sits on a screen with the first star centered: in its column of
    /// `COLUMNS`, below the loose cards above it; a column with no loose cards takes no room.
    fn home(&self, name: &str) -> (f32, f32) {
        let loose = |c: &str| self.visible().iter().any(|v| v == c) && !self.pin.contains_key(c);
        let col = COLUMNS.iter().position(|c| c.contains(&name)).unwrap_or(1);
        let width = |c: &[&str]| c.iter().filter(|n| loose(n)).map(|n| Self::card_size(n).0).fold(0.0, f32::max);
        let x = MARGIN + COLUMNS[..col].iter().filter(|c| width(c) > 0.0).map(|c| width(c) + GAP).sum::<f32>();
        let above = COLUMNS[col].iter().take_while(|n| **n != name).filter(|n| loose(n));
        (x, TOP + above.map(|n| Self::card_size(n).1 + GAP).sum::<f32>())
    }

    /// A card at a world point, as a screen rect, if any of it is on screen.
    fn in_view(v: &View, world: (f32, f32), cw: f32, ch: f32, w: f32, h: f32) -> Option<(f32, f32, f32, f32)> {
        let (x, y) = (world.0 * v.zoom - v.cam.0, world.1 * v.zoom - v.cam.1);
        (x < w && y < h && x + cw > 0.0 && y + ch > 0.0).then_some((x, y, cw, ch))
    }

    pub fn rects(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, f32)> {
        self.visible().iter().filter_map(|c| self.rect(c, w, h)).collect()
    }

    /// Pick up the topmost card under the pointer and raise it. True if there was one.
    pub fn press(&mut self, px: f32, py: f32, w: f32, h: f32) -> bool {
        let hit = self.visible().into_iter().rev().find_map(|c| {
            let (x, y, cw, ch) = self.rect(&c, w, h)?;
            ((x..x + cw).contains(&px) && (y..y + ch).contains(&py)).then_some((c, x, y))
        });
        let Some((name, x, y)) = hit else { return false };
        self.drag = Some(Drag { name: name.clone(), grab: (px - x, py - y), at: (x, y), from: (x, y) });
        self.cards.retain(|c| *c != name);
        self.cards.push(name);
        true
    }

    /// Right-click on a card: its pin moves to the next star (same offset there), so the card leaves this one.
    pub fn send_next(&mut self, px: f32, py: f32, w: f32, h: f32) -> Option<(String, Pin)> {
        let name = self.visible().into_iter().rev().find(|c| self.rect(c, w, h).is_some_and(|(x, y, cw, ch)| (x..x + cw).contains(&px) && (y..y + ch).contains(&py)))?;
        let v = self.view.as_ref()?;
        let at = self.pin.get(&name).map_or(0, |p| v.stars.iter().position(|(n, ..)| *n == p.star).unwrap_or(0));
        let (_, sx, sy) = v.stars.get(at)?;
        let (dx, dy) = match self.pin.get(&name) {
            Some(p) => (p.dx, p.dy),
            None => {
                let (x, y, ..) = self.rect(&name, w, h)?;
                ((x + v.cam.0) / v.zoom - sx, (y + v.cam.1) / v.zoom - sy)
            }
        };
        let next = v.stars.get((at + 1) % v.stars.len()).filter(|_| v.stars.len() > 1)?.0.clone();
        let pin = Pin { constellation: v.constellation.clone(), star: next, dx, dy };
        self.pin.insert(name.clone(), pin.clone());
        Some((name, pin))
    }

    /// A press and release on one of the media card's buttons without moving: the playerctl command to run.
    /// The card is let go of, so it is not pinned.
    pub fn tap(&mut self) -> Option<&'static str> {
        let d = self.drag.as_ref()?;
        let moved = (d.at.0 - d.from.0).abs() + (d.at.1 - d.from.1).abs();
        let (gx, gy) = (d.grab.0 / self.scale(), d.grab.1 / self.scale());
        let hit = d.name == "media" && moved < 4.0 && (gy - MEDIA_BTN_Y).abs() < 16.0 && self.media.is_some();
        let (cw, _) = Self::card_size("media");
        let cmd = hit.then(|| BUTTONS.iter().find(|(dx, _)| (gx - (cw / 2.0 + dx)).abs() < 20.0).map(|b| b.1)).flatten()?;
        self.drag = None;
        Some(cmd)
    }

    /// Move the held card with the pointer. True if a card is held.
    pub fn motion(&mut self, px: f32, py: f32) -> bool {
        let Some(d) = &mut self.drag else { return false };
        d.at = (px - d.grab.0, py - d.grab.1);
        true
    }

    /// Let go: the card is pinned to the star the camera is on, and returned to be remembered.
    /// With no star to pin to (no state from the compositor yet) it goes back where it was.
    pub fn release(&mut self, w: f32, h: f32) -> Option<(String, Pin)> {
        let d = self.drag.take()?;
        let (cw, ch) = (Self::card_size(&d.name).0 * self.scale(), Self::card_size(&d.name).1 * self.scale());
        let (x, y) = (d.at.0.clamp(0.0, (w - cw).max(0.0)), d.at.1.clamp(0.0, (h - ch).max(0.0)));
        let v = self.view.as_ref()?;
        let &(_, sx, sy) = v.stars.iter().find(|(n, _, _)| *n == v.current)?;
        let pin = Pin { constellation: v.constellation.clone(), star: v.current.clone(), dx: (x + v.cam.0) / v.zoom - sx, dy: (y + v.cam.1) / v.zoom - sy };
        self.pin.insert(d.name.clone(), pin.clone());
        Some((d.name, pin))
    }

    pub fn render(&mut self, w: usize, h: usize) -> Canvas {
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let key = self.key();
        if key != self.cache_key {
            self.cache.clear();
            self.cache_key = key;
        }
        for name in self.visible() {
            let Some((x, y, _, _)) = self.rect(&name, w as f32, h as f32) else { continue };
            if !self.cache.contains_key(&name) {
                let card = self.render_card(&name);
                self.cache.insert(name.clone(), card);
            }
            let z = self.scale();
            blit_scaled(&mut cv, &self.cache[&name], (x - CARD_PAD as f32 * z).round() as i32, y.round() as i32, z);
        }
        cv
    }

    /// One card on its own canvas, CARD_PAD wide on the left and right for its outline.
    fn render_card(&mut self, name: &str) -> Canvas {
        let (cw, ch) = Self::card_size(name);
        let mut cv = Canvas { w: cw as usize + 2 * CARD_PAD, h: ch as usize + 2, px: vec![0; (cw as usize + 2 * CARD_PAD) * (ch as usize + 2)] };
        let (x, p) = (CARD_PAD as f32, palette());
        cv.round_rect(x - 1.0, 0.0, cw + 2.0, ch + 1.0, 23.0, rgba(p.text, 0.10));
        cv.round_rect(x, 1.0, cw, ch - 1.0, 22.0, rgba(p.panel, 0.86));
        cv.round_rect(x, 1.0, cw, ch - 1.0, 22.0, rgba(p.text, 0.03));
        match name {
            "clock" => self.clock(&mut cv, x, cw, ch),
            "calendar" => self.calendar(&mut cv, x, cw),
            "media" => self.media(&mut cv, x, cw),
            "weather" => self.weather_card(&mut cv, x, cw),
            "agenda" => self.agenda_card(&mut cv, x, cw),
            _ => self.system(&mut cv, x, cw),
        }
        cv
    }

    fn clock(&mut self, cv: &mut Canvas, x: f32, cw: f32, ch: f32) {
        let p = palette();
        let (_, _, _, _, t) = now_parts();
        self.t64.draw(cv, x + 26.0, 82.0, &local_time(seiza_common::clock_fmt()), rgba(p.text, 1.0));
        self.t18.draw(cv, x + 28.0, ch - 26.0, &local_time(c"%a %d %b").to_lowercase(), rgba(p.text, 0.6));
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
        self.t14.draw(cv, x + cw - 24.0 - nw, ch - 26.0, name, rgba(p.text, 0.5));
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
                    cv.round_rect(cx - 14.0, cy - 14.0, 28.0, 28.0, 14.0, rgba(p.star, 0.85)); // indigo with a gold outline
                    cv.round_rect(cx - 13.0, cy - 13.0, 26.0, 26.0, 13.0, rgba(p.dust, 1.0));
                    self.t14.draw(cv, cx - sw / 2.0, cy + 5.0, &s, rgba(p.text, 1.0));
                } else {
                    self.t14.draw(cv, cx - sw / 2.0, cy + 5.0, &s, rgba(p.text, if c >= 5 { 0.55 } else { 0.85 }));
                }
            }
        }
    }

    fn media(&mut self, cv: &mut Canvas, x: f32, cw: f32) {
        let p = palette();
        self.t14.draw(cv, x + 24.0, 38.0, "now playing", rgba(p.text, 0.5));
        let Some(m) = self.media.clone() else {
            self.t18.draw(cv, x + 24.0, 84.0, "nothing playing", rgba(p.text, 0.4));
            return;
        };
        let state = if m.playing { "playing" } else { "paused" };
        let sw = self.t14.width(state);
        self.t14.draw(cv, x + cw - 24.0 - sw, 38.0, state, rgba(p.text, 0.4));
        let art = self.art.1.is_some();
        if let Some(a) = &self.art.1 {
            blit(cv, a, (x + 24.0) as i32, 52);
        }
        let (tx, room) = if art { (x + 24.0 + ART as f32 + 12.0, cw - 48.0 - ART as f32 - 12.0) } else { (x + 24.0, cw - 48.0) };
        let title = fit(&mut self.t18, &m.title, room);
        self.t18.draw(cv, tx, 78.0, &title, rgba(p.text, if m.playing { 1.0 } else { 0.7 }));
        let artist = fit(&mut self.t14, &m.artist, room);
        self.t14.draw(cv, tx, 102.0, &artist, rgba(p.text, 0.6));
        let (bx, by, ink) = (x + cw / 2.0, MEDIA_BTN_Y, rgba(p.text, 0.8));
        cv.shape(bx - 44.0, by, 8.0, ink, |px, py| { let u = px - (bx - 44.0); (u >= -8.0 && u <= -5.0) || (u > -5.0 && (py - by).abs() <= (u + 5.0) * 0.6) });
        if m.playing {
            cv.round_rect(bx - 7.0, by - 8.0, 5.0, 16.0, 1.5, ink);
            cv.round_rect(bx + 2.0, by - 8.0, 5.0, 16.0, 1.5, ink);
        } else {
            cv.shape(bx, by, 9.0, ink, |px, py| px - bx >= -6.0 && px - bx <= 8.0 && (py - by).abs() <= (8.0 - (px - bx)) * 0.57);
        }
        cv.shape(bx + 44.0, by, 8.0, ink, |px, py| { let u = px - (bx + 44.0); (u >= 5.0 && u <= 8.0) || (u <= 5.0 && u >= -8.0 && (py - by).abs() <= (5.0 - u) * 0.6) });
        if m.len > 0 {
            // browsers often report no length and a stuck position, so there is no bar without a length
            cv.round_rect(x + 24.0, 125.0, room, 6.0, 3.0, rgba(p.dust, 0.9));
            let f = (m.pos as f32 / m.len as f32).clamp(0.0, 1.0);
            cv.round_rect(x + 24.0, 125.0, (room * f).max(6.0), 6.0, 3.0, rgba(p.star, if m.playing { 0.9 } else { 0.5 }));
            let total = clock(m.len);
            let tw = self.t14.width(&total);
            self.t14.draw(cv, x + cw - 24.0 - tw, 154.0, &total, rgba(p.text, 0.45));
            self.t14.draw(cv, x + 24.0, 154.0, &clock(m.pos), rgba(p.text, 0.45));
        }
    }

    fn weather_card(&mut self, cv: &mut Canvas, x: f32, cw: f32) {
        let p = palette();
        self.t14.draw(cv, x + 24.0, 38.0, "weather", rgba(p.text, 0.5));
        let Some(w) = self.weather.clone() else {
            self.t18.draw(cv, x + 24.0, 84.0, "no weather yet", rgba(p.text, 0.4));
            return;
        };
        self.t64.draw(cv, x + 24.0, 102.0, &w.temp, rgba(p.text, 1.0));
        let room = (cw - 48.0 - self.t64.width(&w.temp) - 14.0).max(40.0); // what the temperature leaves, and a gap
        let t = if self.t18.width(&w.sky) <= room { &mut self.t18 } else { &mut self.t14 }; // small text before an ellipsis
        let sky = fit(t, &w.sky, room);
        let sw = t.width(&sky);
        t.draw(cv, x + cw - 24.0 - sw, 90.0, &sky, rgba(p.text, 0.85));
        let place = fit(&mut self.t14, &w.place, cw - 48.0);
        self.t14.draw(cv, x + 24.0, 126.0, &place, rgba(p.text, 0.6));
        let extra = fit(&mut self.t14, &w.extra, cw - 48.0);
        let ew = self.t14.width(&extra);
        self.t14.draw(cv, x + cw - 24.0 - ew, 38.0, &extra, rgba(p.text, 0.4));
    }

    fn agenda_card(&mut self, cv: &mut Canvas, x: f32, cw: f32) {
        let p = palette();
        self.t14.draw(cv, x + 24.0, 38.0, "coming up", rgba(p.text, 0.5));
        match self.agenda.clone() {
            None => self.t14.draw(cv, x + 24.0, 84.0, "set calendar.source in seiza.kdl", rgba(p.text, 0.4)),
            Some(rows) if rows.is_empty() => self.t18.draw(cv, x + 24.0, 84.0, "nothing coming up", rgba(p.text, 0.4)),
            Some(rows) => {
                for (i, (when, what)) in rows.iter().enumerate() {
                    let y = 72.0 + i as f32 * 32.0;
                    self.t14.draw(cv, x + 24.0, y, when, rgba(p.star, 0.9));
                    let what = fit(&mut self.t18, what, cw - 48.0 - 110.0);
                    self.t18.draw(cv, x + 24.0 + 110.0, y + 1.0, &what, rgba(p.text, 0.95));
                }
                0.0
            }
        };
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

    fn view(current: &str, cam: (f32, f32)) -> View {
        // two stars 2000 apart; with a star centered on a 1920x1080 screen the camera is its center minus (960, 540)
        View { constellation: "default".into(), current: current.into(), stars: vec![("a".into(), 960.0, 540.0), ("b".into(), 2960.0, 540.0)], zoom: 1.0, cam }
    }

    #[test]
    fn cards_shrink_with_the_zoom_like_windows() {
        let mut ui = Ui::new(vec!["clock".into()]);
        let mut v = view("a", (0.0, 0.0));
        let (_, _, w1, h1) = { ui.view = Some(v.clone()); ui.rect("clock", 1920.0, 1080.0).unwrap() };
        v.zoom = 0.5;
        ui.view = Some(v);
        let (_, _, w, h) = ui.rect("clock", 1920.0, 1080.0).unwrap();
        assert_eq!((w, h), (w1 / 2.0, h1 / 2.0));
        let mut cv = Canvas { w: 400, h: 300, px: vec![0; 400 * 300] };
        blit_scaled(&mut cv, &Canvas { w: 100, h: 100, px: vec![0xFF00_FF00; 100 * 100] }, 10, 10, 0.5);
        assert_eq!((cv.px[30 * 400 + 30], cv.px[70 * 400 + 70]), (0xFF00_FF00, 0), "filled inside 50x50, empty outside");
    }

    #[test]
    fn cards_start_in_two_columns_and_a_dragged_one_is_pinned_to_its_star() {
        let mut ui = Ui::new(["clock", "calendar", "system"].map(String::from).to_vec());
        let (w, h) = (1920.0, 1080.0);
        // the first column: clock, then the calendar under it; the second column starts beside them
        assert_eq!(ui.rect("clock", w, h), Some((MARGIN, TOP, 340.0, 150.0)));
        assert_eq!(ui.rect("calendar", w, h).map(|r| (r.0, r.1)), Some((MARGIN, TOP + 150.0 + GAP)));
        assert_eq!(ui.rect("system", w, h).map(|r| (r.0, r.1)), Some((MARGIN + 340.0 + GAP, TOP)));
        assert!(ui.rects(w, h).iter().all(|r| r.0 + r.2 < w - 400.0), "the top-right is left for the toasts");
        assert!(!ui.press(5.0, 5.0, w, h), "empty sky is not a card");
        ui.view = Some(view("a", (0.0, 0.0)));
        let (cx, cy, _, _) = ui.rect("clock", w, h).unwrap();
        assert!(ui.press(cx + 10.0, cy + 20.0, w, h));
        assert_eq!(ui.cards.last().unwrap(), "clock", "the held card is raised");
        assert!(ui.motion(cx + 110.0, cy + 120.0));
        assert_eq!(ui.rect("clock", w, h).unwrap().0, cx + 100.0, "it follows the pointer while held");
        let (name, pin) = ui.release(w, h).unwrap();
        assert_eq!((name.as_str(), pin.star.as_str()), ("clock", "a"));
        assert_eq!((pin.dx, pin.dy), (cx + 100.0 - 960.0, cy + 100.0 - 540.0), "an offset from the star's center");
        assert_eq!(ui.rect("clock", w, h).unwrap().0, cx + 100.0);
        assert_eq!(ui.rect("calendar", w, h).map(|r| (r.0, r.1)), Some((MARGIN, TOP)), "the column closes up around the one that left");
        assert!(!ui.motion(1.0, 1.0), "nothing is held any more");
    }

    #[test]
    fn a_column_with_no_cards_takes_no_room() {
        let ui = Ui::new(["system", "media"].map(String::from).to_vec());
        assert_eq!(ui.rect("system", 1920.0, 1080.0).map(|r| (r.0, r.1)), Some((MARGIN, TOP)), "no clock or calendar: system takes the first column");
        assert_eq!(ui.rect("media", 1920.0, 1080.0).map(|r| (r.0, r.1)), Some((MARGIN, TOP + 236.0 + GAP)));
    }

    #[test]
    fn a_pinned_card_follows_the_camera_and_is_gone_on_other_stars() {
        let mut ui = Ui::new(vec!["clock".into()]);
        let (w, h) = (1920.0, 1080.0);
        ui.view = Some(view("a", (0.0, 0.0)));
        ui.pin.insert("clock".into(), Pin { constellation: "default".into(), star: "a".into(), dx: -500.0, dy: -300.0 });
        assert_eq!(ui.rect("clock", w, h).map(|r| (r.0, r.1)), Some((460.0, 240.0)));
        ui.view.as_mut().unwrap().cam = (100.0, 40.0); // the camera pans: the card slides the other way
        assert_eq!(ui.rect("clock", w, h).map(|r| (r.0, r.1)), Some((360.0, 200.0)));
        ui.view = Some(view("b", (2000.0, 0.0))); // the camera is on star b: star a's cards are off to the left
        assert_eq!(ui.rect("clock", w, h), None);
        ui.view.as_mut().unwrap().constellation = "other".into();
        assert_eq!(ui.rect("clock", w, h), None, "and another constellation has none of them");
        assert!(ui.rects(w, h).is_empty());
        assert!(!ui.press(500.0, 300.0, w, h), "a card that is not there cannot be grabbed");
    }

    #[test]
    fn cards_never_dragged_are_on_the_first_star_only() {
        let mut ui = Ui::new(vec!["clock".into()]);
        let (w, h) = (1920.0, 1080.0);
        ui.view = Some(view("a", (0.0, 0.0)));
        assert_eq!(ui.rect("clock", w, h).map(|r| (r.0, r.1)), Some((MARGIN, TOP)));
        ui.view = Some(view("b", (2000.0, 0.0))); // star 2: a clean screen
        assert_eq!(ui.rect("clock", w, h), None);
        assert!(ui.rects(w, h).is_empty());
    }

    #[test]
    fn letting_go_with_no_star_to_pin_to_changes_nothing() {
        let mut ui = Ui::new(vec!["clock".into()]);
        assert!(ui.press(100.0, 100.0, 1920.0, 1080.0));
        ui.motion(900.0, 400.0);
        assert!(ui.release(1920.0, 1080.0).is_none());
        assert!(ui.pin.is_empty());
    }

    /// cargo test --release -p seiza-widgets how_long -- --ignored --nocapture
    #[test]
    #[ignore]
    fn how_long_a_dragged_frame_takes() {
        let mut ui = Ui::new(CARDS.map(String::from).to_vec());
        ui.render(1920, 1080); // the first frame draws every card
        let t = std::time::Instant::now();
        for i in 0..100 {
            ui.view = Some(view("a", (0.0, 0.0)));
            ui.press(100.0, 100.0, 1920.0, 1080.0);
            ui.motion(100.0 + i as f32, 100.0 + i as f32);
            ui.render(1920, 1080);
            ui.release(1920.0, 1080.0);
        }
        println!("a dragged frame at 1080p: {:?}", t.elapsed() / 100);
    }

    #[test]
    fn right_click_sends_a_card_to_the_next_star() {
        let (w, h) = (1920.0, 1080.0);
        let mut ui = Ui::new(vec!["clock".into()]);
        let v = view("a", (0.0, 0.0));
        ui.view = Some(v);
        let (x, y, cw, _) = ui.rect("clock", w, h).unwrap();
        let (name, pin) = ui.send_next(x + cw / 2.0, y + 5.0, w, h).unwrap();
        assert_eq!((name.as_str(), pin.star.as_str()), ("clock", "b"));
        assert!(ui.rect("clock", w, h).is_none(), "gone from this star");
        assert!(ui.send_next(x, y, w, h).is_none(), "nothing under the pointer now");
    }

    #[test]
    fn tapping_a_media_button_runs_it_and_dragging_does_not() {
        let (w, h) = (1920.0, 1080.0);
        let mut ui = Ui::new(vec!["media".into()]);
        ui.media = Some(Media { playing: true, title: "t".into(), artist: "a".into(), pos: 1, len: 10, art: String::new() });
        let (x, y, cw, _) = ui.rect("media", w, h).unwrap();
        assert!(ui.press(x + cw / 2.0 + 44.0, y + MEDIA_BTN_Y, w, h));
        assert_eq!(ui.tap(), Some("next"));
        assert!(ui.press(x + 10.0, y + MEDIA_BTN_Y, w, h), "off the buttons it is a drag");
        assert_eq!(ui.tap(), None);
        assert!(ui.press(x + cw / 2.0, y + MEDIA_BTN_Y, w, h));
        ui.motion(x + cw / 2.0 + 30.0, y + MEDIA_BTN_Y);
        assert_eq!(ui.tap(), None, "moved: a drag");
    }

    #[test]
    fn media_prefers_the_playing_player_and_skips_stopped_ones() {
        let line = |st: &str, t: &str| format!("{st}\u{1f}{t}\u{1f}Art\u{1f}83000000\u{1f}240000000\u{1f}file:///tmp/a%20b.png\n");
        let m = parse_media(&(line("Paused", "a") + &line("Playing", "b"))).unwrap();
        assert_eq!((m.title.as_str(), m.playing, m.pos, m.len), ("b", true, 83, 240));
        assert_eq!(parse_media(&line("Paused", "a")).unwrap().title, "a");
        assert!(parse_media(&line("Stopped", "a")).is_none());
        assert!(parse_media("").is_none());
        assert_eq!(parse_media(&line("Playing", "a")).unwrap().art, "/tmp/a b.png");
        assert_eq!(clock(83), "1:23");
    }

    #[test]
    fn the_moon_is_full_on_a_known_full_moon_day() {
        // 2026-10-26 is a full moon (about 04:12 UTC)
        let age = moon_age(1_792_988_000.0);
        assert!((age - 14.77).abs() < 1.0, "age {age}");
        assert_eq!(moon_name(age), "full moon");
    }
}
