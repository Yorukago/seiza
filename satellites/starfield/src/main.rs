//! seiza-starfield: a parallax starfield wallpaper for Seiza.
//!
//! Draws into a layer-shell background surface and follows the camera that
//! `seiza subscribe` streams, so the sky drifts as you pan and spreads out as
//! you zoom. Deeper layers move less than nearer ones.
use std::{
    thread,
    time::{Duration, Instant},
};

use seiza_common::{
    palette::palette,
    art,
    draw::{load_font, Canvas, Text},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            channel,
            timer::{TimeoutAction, Timer},
            EventLoop,
        },
        calloop_wayland_source::WaylandSource,
        client::{
            globals::registry_queue_init,
            protocol::{wl_output, wl_shm, wl_surface},
            Connection, QueueHandle,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

const FRAME: Duration = Duration::from_millis(33);

/// What we need from the compositor's state line.
#[derive(Clone, Copy)]
struct Cam {
    x: f64,
    y: f64,
    zoom: f64,
    screen: (f64, f64),
}

/// A Seiza star (workspace), in world coordinates.
#[derive(Clone)]
struct SkyStar {
    name: String,
    x: f64,
    y: f64,
    windows: u64,
    current: bool,
}

/// One state line: the camera and the stars.
struct Msg {
    overview: bool,
    cam: Cam,
    stars: Vec<SkyStar>,
    links: Vec<(usize, usize)>,
}

/// An inked thread between two points: a gentle arc, drawn twice with a small wobble.
fn thread(px: &mut [u32], w: usize, h: usize, a: (f32, f32), b: (f32, f32), seed: u64, color: [f32; 3]) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy).max(1.0);
    let bend = len * (0.08 + (seed % 5) as f32 * 0.02) * if seed % 2 == 0 { 1.0 } else { -1.0 };
    let ctrl = ((a.0 + b.0) / 2.0 - dy / len * bend, (a.1 + b.1) / 2.0 + dx / len * bend);
    for pass in 0..2 {
        let steps = (len / 1.6).ceil() as i32; // close enough that the dots read as one line
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let (u, v) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t));
            let wob = if pass == 0 { 0.0 } else { 1.8 * (t * 11.0 + seed as f32).sin() + 1.4 };
            let x = u * a.0 + v * ctrl.0 + t * t * b.0 - dy / len * wob;
            let y = u * a.1 + v * ctrl.1 + t * t * b.1 + dx / len * wob;
            let alpha = if pass == 0 { 1.0 } else { 0.45 };
            if s % 3 == 0 {
                glow(px, w, h, x, y, 3.2, color, 0.18 * alpha, false); // soft halo: every third dot is enough, they overlap
            }
            glow(px, w, h, x, y, 1.0, [1.0, 0.97, 0.85], 0.4 * alpha, false); // thin bright core
        }
    }
}

/// A meteor crossing the screen.
struct Shooter {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    age: f32,
    life: f32,
    color: [f32; 3],
}

/// Stream camera updates from the compositor into the event loop.
fn read_state(tx: channel::Sender<Msg>) {
    seiza_common::subscribe(|st| {
        let cam = Cam { x: st.cam.0, y: st.cam.1, zoom: st.zoom, screen: st.screen };
        let stars = st
            .stars
            .iter()
            .map(|s| SkyStar { name: s.name.clone(), x: s.x, y: s.y, windows: s.windows, current: s.name == st.current })
            .collect();
        tx.send(Msg { overview: st.overview, cam, stars, links: st.links }).is_ok() // false once the main loop is gone
    });
}

// ---- noise and stars ------------------------------------------------------

fn hash(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn unit(h: u64, shift: u32) -> f32 {
    ((h >> shift) & 0xFFFF) as f32 / 65535.0
}

fn vnoise(x: f32, y: f32, seed: u64) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (u, v) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let c = |dx: i64, dy: i64| unit(hash(seed ^ ((xi as i64 + dx) as u64).wrapping_mul(73856093) ^ ((yi as i64 + dy) as u64).wrapping_mul(19349663)), 0);
    let top = c(0, 0) + (c(1, 0) - c(0, 0)) * u;
    let bot = c(0, 1) + (c(1, 1) - c(0, 1)) * u;
    top + (bot - top) * v
}

fn fbm(x: f32, y: f32, seed: u64) -> f32 {
    (0.5 * vnoise(x, y, seed) + 0.25 * vnoise(x * 2.0, y * 2.0, seed) + 0.125 * vnoise(x * 4.0, y * 4.0, seed)) / 0.875
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How far past the screen the nebula is painted, in pixels.
const NEBULA_MARGIN: usize = 256;

/// (depth: how much of the world's movement this layer follows, tile size in
/// sky pixels, stars per tile, base radius in pixels, brightness).
const LAYERS: [(f32, f32, u64, f32, f32); 3] = [
    (0.08, 260.0, 12, 0.9, 0.55),
    (0.22, 340.0, 8, 1.3, 0.8),
    (0.50, 460.0, 4, 1.9, 1.0),
];

fn add(px: &mut u32, c: [f32; 3], a: f32) {
    let ch = |shift: u32, v: f32| ((*px >> shift) & 0xFF).min(255) as f32 + v * a * 255.0;
    let (r, g, b) = (ch(16, c[0]), ch(8, c[1]), ch(0, c[2]));
    *px = 0xFF00_0000 | (r.min(255.0) as u32) << 16 | (g.min(255.0) as u32) << 8 | b.min(255.0) as u32;
}

fn glow(frame: &mut [u32], w: usize, h: usize, x: f32, y: f32, r: f32, c: [f32; 3], a: f32, rays: bool) {
    let reach = if rays { r * 5.0 } else { r * 2.5 };
    let (x0, x1) = ((x - reach).floor().max(0.0) as usize, ((x + reach).ceil() as usize).min(w.saturating_sub(1)));
    let (y0, y1) = ((y - reach).floor().max(0.0) as usize, ((y + reach).ceil() as usize).min(h.saturating_sub(1)));
    if x1 < x0 || y1 < y0 || x < -reach || y < -reach {
        return;
    }
    for py in y0..=y1 {
        for px in x0..=x1 {
            let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
            let d = (dx * dx + dy * dy).sqrt();
            let mut k = (1.0 - d / (r * 2.5)).max(0.0).powi(2);
            if rays {
                let along = dx.abs().min(dy.abs()); // distance from the nearest axis
                k += (1.0 - along / (r * 0.6)).max(0.0) * (1.0 - d / reach).max(0.0).powi(2) * 0.8;
            }
            if k > 0.0 {
                add(&mut frame[py * w + px], c, k * a);
            }
        }
    }
}

// ---- the app --------------------------------------------------------------

struct Starfield {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    width: u32,
    height: u32,
    configured: bool,
    frame_pending: bool,
    exit: bool,
    target: Cam,
    cur: Cam,
    at: (f64, f64), // where this monitor sits in the compositor's layout: 0,0 for the primary one
    sky_stars: Vec<SkyStar>,
    sky_links: Vec<(usize, usize)>,
    overview: bool,
    art: Option<art::Match>, // a figure the stars currently form
    art_key: Vec<(i64, i64)>,
    art_a: f32,              // 0..1, eases in when a figure appears
    text: Text,
    shooters: Vec<Shooter>,
    next_shot: f32,
    rng: u64,
    shooting: bool, // `shooting`, `labels`, `threads` in ~/.config/seiza/sky: each on unless switched off
    labels: bool,
    threads: bool,
    nebula_gain: f32, // SEIZA_STARFIELD_NEBULA or `nebula=` in ~/.config/seiza/sky: default 0.4; 1 = full colour, 0 = black sky
    bg: Vec<u32>,
    spent: Duration, // SEIZA_STARFIELD_TIMING: time spent drawing, over `frames` frames
    frames: u32,
    bg_origin: (f32, f32), // the parallax offset the nebula was made for
    bg_size: (usize, usize),
    frame: Vec<u32>,
    start: Instant,
    last: Instant,
}

impl Starfield {
    /// The nebula is painted onto a canvas a margin larger than the screen, and a pan only
    /// slides the screen over it: repainting costs tens of milliseconds, which is what made
    /// the glide into the overview choppy. Returns how far to slide, in pixels; it is repainted
    /// only when the slide would run out of margin.
    fn nebula(&mut self, wcx: f64, wcy: f64) -> (i32, i32) {
        let (w, h) = (self.width as usize, self.height as usize);
        let (bw, bh) = (w + 2 * NEBULA_MARGIN, h + 2 * NEBULA_MARGIN);
        let (ox, oy) = (wcx as f32 * 0.04, wcy as f32 * 0.04);
        let room = (NEBULA_MARGIN - 16) as f32;
        let fresh = self.bg.len() == bw * bh && self.bg_size == (w, h) && (ox - self.bg_origin.0).abs() < room && (oy - self.bg_origin.1).abs() < room;
        if fresh {
            return ((ox - self.bg_origin.0).round() as i32, (oy - self.bg_origin.1).round() as i32);
        }
        self.bg_origin = (ox, oy);
        self.bg_size = (w, h);
        let (w, h) = (bw, bh);
        // Sample at 1/6 resolution, then upscale: a nebula is soft anyway.
        const D: usize = 6;
        let (lw, lh) = (w / D + 2, h / D + 2);
        let sc = 1.0 / 650.0;
        let mut low = vec![[0f32; 3]; lw * lh];
        for ly in 0..lh {
            for lx in 0..lw {
                let (sx, sy) = ((lx * D) as f32 - NEBULA_MARGIN as f32 + ox, (ly * D) as f32 - NEBULA_MARGIN as f32 + oy);
                // gouache: warp the coordinates so edges wander, and keep the cloud edges tight and soft
                let (wx, wy) = (sx + 140.0 * (fbm(sx * sc * 0.6, sy * sc * 0.6, 3) - 0.5), sy + 140.0 * (fbm(sx * sc * 0.6 + 9.0, sy * sc * 0.6, 4) - 0.5));
                let a = smooth(0.50, 0.66, fbm(wx * sc, wy * sc, 11));
                let b = smooth(0.52, 0.70, fbm(wx * sc + 40.0, wy * sc * 1.2, 23));
                let c = smooth(0.46, 0.64, fbm(wx * sc * 0.7 - 17.0, wy * sc * 0.7, 37));
                let ember = smooth(0.62, 0.9, fbm(sx * sc * 1.4 + 90.0, sy * sc * 1.4 - 30.0, 51));
                let pal = palette();
                let mut px = pal.sky.map(|v| v * self.nebula_gain.max(1.0));
                for (col, amt) in [(pal.dust, a * 0.60), (pal.mist, b * 0.50), (pal.glow, c * 0.35), (pal.star, ember * 0.10)] {
                    for i in 0..3 {
                        px[i] += col[i] * amt * self.nebula_gain;
                    }
                }
                low[ly * lw + lx] = px;
            }
        }
        self.bg.resize(w * h, 0);
        for y in 0..h {
            let (fy, ly) = ((y % D) as f32 / D as f32, y / D);
            for x in 0..w {
                let (fx, lx) = ((x % D) as f32 / D as f32, x / D);
                let p = |dx: usize, dy: usize| low[(ly + dy) * lw + lx + dx];
                let (p00, p10, p01, p11) = (p(0, 0), p(1, 0), p(0, 1), p(1, 1));
                let mut out = 0xFF00_0000u32;
                for i in 0..3 {
                    let t = p00[i] + (p10[i] - p00[i]) * fx;
                    let bt = p01[i] + (p11[i] - p01[i]) * fx;
                    let grain = (unit(hash((y * w + x) as u64 * 3 + i as u64), 0) - 0.5) * 0.02; // paper grain, fixed to the screen
                    let v = ((t + (bt - t) * fy + grain).clamp(0.0, 1.0) * 255.0) as u32;
                    out |= v << (16 - 8 * i);
                }
                self.bg[y * w + x] = out;
            }
        }
        (0, 0)
    }

    fn stars(&mut self, wcx: f64, wcy: f64, t: f32) {
        let (w, h) = (self.width as usize, self.height as usize);
        for (li, &(depth, tile, per_tile, radius, bright)) in LAYERS.iter().enumerate() {
            let k = self.cur.zoom.powf(depth as f64) as f32; // far layers barely scale
            let (cx, cy) = (wcx as f32 * depth, wcy as f32 * depth);
            let (hx, hy) = (w as f32 / 2.0 / k + 12.0, h as f32 / 2.0 / k + 12.0);
            let (tx0, tx1) = (((cx - hx) / tile).floor() as i64, ((cx + hx) / tile).floor() as i64);
            let (ty0, ty1) = (((cy - hy) / tile).floor() as i64, ((cy + hy) / tile).floor() as i64);
            for ty in ty0..=ty1 {
                for tx in tx0..=tx1 {
                    for i in 0..per_tile {
                        let hv = hash(((tx as u64).wrapping_mul(0x9E37_79B1) ^ (ty as u64).wrapping_mul(0x85EB_CA6B)) ^ ((li as u64) << 56) ^ (i << 48));
                        let (sx, sy) = ((tx as f32 + unit(hv, 0)) * tile, (ty as f32 + unit(hv, 16)) * tile);
                        let (x, y) = ((sx - cx) * k + w as f32 / 2.0, (sy - cy) * k + h as f32 / 2.0);
                        let color = seiza_common::STAR_COLORS[(unit(hv, 32) * 5.999) as usize];
                        let size = radius * (0.6 + unit(hv, 40) * 0.9);
                        let tw = 0.65 + 0.35 * (t * (0.8 + unit(hv, 48) * 2.2) + unit(hv, 56) * 6.28).sin();
                        let big = unit(hv, 24) > 0.88;
                        glow(&mut self.frame, w, h, x, y, size * if big { 1.3 } else { 1.0 }, color, bright * tw, big && li > 0);
                    }
                }
            }
        }
    }

    /// The workspaces, glowing where they really are (exact camera, no glide, so
    /// they line up with the overview markers). Busier stars burn brighter.
    fn seiza_stars(&mut self, t: f32) {
        let (w, h) = (self.width as usize, self.height as usize);
        let c = self.target;
        let z = c.zoom as f32;
        for (i, st) in self.sky_stars.iter().enumerate() {
            let (x, y) = ((st.x * c.zoom - c.x) as f32, (st.y * c.zoom - c.y) as f32);
            let busy = st.windows.min(6) as f32;
            let halo = (110.0 + busy * 20.0) * z.sqrt();
            let pulse = if st.current { 0.85 + 0.15 * (t * 1.6).sin() } else { 1.0 };
            let color = seiza_common::STAR_COLORS[i % 6];
            glow(&mut self.frame, w, h, x, y, halo, color, (0.55 + busy * 0.06) * pulse, false);
            glow(&mut self.frame, w, h, x, y, (5.0 + busy) * z.sqrt().max(0.6), [1.0, 1.0, 1.0], pulse, true);
        }
    }

    /// The constellation view: star names, and the art when the stars form a figure.
    fn constellation_view(&mut self, t: f32) {
        let (w, h) = (self.width as usize, self.height as usize);
        let c = self.target;
        let to_screen = |x: f64, y: f64| ((x * c.zoom - c.x) as f32, (y * c.zoom - c.y) as f32);
        let mut cv = Canvas { w, h, px: std::mem::take(&mut self.frame) };

        if let (Some(m), true) = (&self.art, self.art_a > 0.01) {
            let a = self.art_a;
            let color = if m.template == 0 { [1.0, 0.86, 0.5] } else { [0.72, 0.78, 1.0] };
            let fig = &art::templates()[m.template];
            for line in &fig.figure {
                for seg in line.windows(2) {
                    let (p0, p1) = (to_screen(m.origin.0 + seg[0].0 * m.scale, m.origin.1 + seg[0].1 * m.scale), to_screen(m.origin.0 + seg[1].0 * m.scale, m.origin.1 + seg[1].1 * m.scale));
                    let len = (p1.0 - p0.0).hypot(p1.1 - p0.1);
                    for i in 0..(len / 3.0).ceil() as usize {
                        let f = i as f32 * 3.0 / len;
                        let (x, y) = (p0.0 + (p1.0 - p0.0) * f, p0.1 + (p1.1 - p0.1) * f);
                        glow(&mut cv.px, w, h, x, y, 4.0, color, 0.16 * a, false); // soft halo
                        glow(&mut cv.px, w, h, x, y, 1.6, [1.0, 1.0, 1.0], 0.8 * a, false); // bright core
                    }
                }
            }
            for &i in &m.stars {
                let s = &self.sky_stars[i];
                let (x, y) = to_screen(s.x, s.y);
                let burst = 1.0 + 0.25 * (t * 2.0).sin();
                glow(&mut cv.px, w, h, x, y, 9.0 * burst, color, a, true);
            }
            let (cx, cy) = to_screen(m.origin.0, m.origin.1 + 0.08 * m.scale);
            let title = if m.template == 0 { "the Star" } else { "the Moon" };
            let tw = self.text.width(title);
            self.text.draw(&mut cv, cx - tw / 2.0, cy, title, [color[0], color[1], color[2], a]);
        }

        // threads between the stars of a figure: hand-inked, doubled, never straight
        for (n, &(a, b)) in self.sky_links.iter().enumerate().filter(|_| self.threads) {
            if let (Some(sa), Some(sb)) = (self.sky_stars.get(a), self.sky_stars.get(b)) {
                let (p0, p1) = (to_screen(sa.x, sa.y), to_screen(sb.x, sb.y));
                thread(&mut cv.px, w, h, p0, p1, n as u64, [0.95, 0.85, 0.55]);
            }
        }

        for st in self.sky_stars.iter().filter(|_| self.labels) {
            let (x, y) = to_screen(st.x, st.y);
            let tw = self.text.width(&st.name);
            self.text.draw(&mut cv, x - tw / 2.0, y + 36.0, &st.name, [1.0, 1.0, 1.0, if st.current { 0.95 } else { 0.6 }]);
        }
        self.frame = cv.px;
    }

    fn shooting_stars(&mut self, dt: f32) {
        if !self.shooting {
            self.shooters.clear();
            return;
        }
        let (w, h) = (self.width as f32, self.height as f32);
        self.next_shot -= dt;
        if self.next_shot <= 0.0 {
            self.rng = hash(self.rng);
            let r = self.rng;
            let dir = if unit(r, 0) < 0.5 { 1.0 } else { -1.0 };
            let ang = (25.0 + unit(r, 16) * 20.0).to_radians();
            let speed = 1100.0 + unit(r, 32) * 600.0;
            self.shooters.push(Shooter {
                x: w * (0.15 + unit(r, 40) * 0.8),
                y: h * unit(r, 48) * 0.4,
                vx: dir * ang.cos() * speed,
                vy: ang.sin() * speed,
                age: 0.0,
                life: 0.7 + unit(r, 8) * 0.4,
                color: seiza_common::STAR_COLORS[(unit(r, 24) * 5.999) as usize],
            });
            self.next_shot = 2.5 + unit(hash(r), 0) * 6.0;
        }
        for s in &mut self.shooters {
            s.age += dt;
            s.x += s.vx * dt;
            s.y += s.vy * dt;
        }
        self.shooters.retain(|s| s.age < s.life);
        for s in &self.shooters {
            let fade = (std::f32::consts::PI * s.age / s.life).sin();
            const N: usize = 150;
            for i in 0..N {
                let f = i as f32 / N as f32;
                let back = i as f32 * 0.0022;
                let tail = [s.color[0] + (1.0 - s.color[0]) * (1.0 - f), s.color[1] + (1.0 - s.color[1]) * (1.0 - f), s.color[2] + (1.0 - s.color[2]) * (1.0 - f)];
                glow(&mut self.frame, self.width as usize, self.height as usize, s.x - s.vx * back, s.y - s.vy * back, 2.2 * (1.0 - f * 0.75), tail, (1.0 - f).powf(1.8) * fade * 0.85, i == 0);
            }
        }
    }

    fn tick(&mut self, qh: &QueueHandle<Self>) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        // Glide toward the compositor's camera: its jumps become soft drifts of the sky.
        let e = 1.0 - (-dt * 9.0).exp();
        let (t, c) = (self.target, &mut self.cur);
        c.x += (t.x - c.x) * e;
        c.y += (t.y - c.y) * e;
        c.zoom = (c.zoom.ln() + (t.zoom.ln() - c.zoom.ln()) * e).exp();
        c.screen = t.screen;
        if !self.configured || self.frame_pending {
            return;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let drawing = Instant::now();
        let (wcx, wcy) = ((self.cur.x + w as f64 / 2.0) / self.cur.zoom, (self.cur.y + h as f64 / 2.0) / self.cur.zoom); // the world point at this monitor's centre
        let (dx, dy) = self.nebula(wcx, wcy);
        self.frame.clear();
        let bw = w + 2 * NEBULA_MARGIN;
        for y in 0..h {
            let at = (y as i32 + dy + NEBULA_MARGIN as i32) as usize * bw + (dx + NEBULA_MARGIN as i32) as usize;
            self.frame.extend_from_slice(&self.bg[at..at + w]);
        }
        let t = self.start.elapsed().as_secs_f32();
        self.stars(wcx, wcy, t);
        self.seiza_stars(t);
        let want = if self.overview && self.art.is_some() { 1.0 } else { 0.0 };
        self.art_a += (want - self.art_a) * (1.0 - (-dt as f32 * 2.5).exp());
        if self.overview {
            self.constellation_view(t);
        }
        self.shooting_stars(dt as f32);
        if std::env::var_os("SEIZA_STARFIELD_TIMING").is_some() { // how long a frame takes to draw, averaged over 120
            self.spent += drawing.elapsed();
            self.frames += 1;
            if self.frames == 120 {
                eprintln!("seiza-starfield: {:?} to draw a frame ({}x{}, overview {})", self.spent / 120, w, h, self.overview);
                (self.spent, self.frames) = (Duration::ZERO, 0);
            }
        }

        if let Ok(path) = std::env::var("SEIZA_STARFIELD_DUMP") { // debugging aid: write a frame as PPM and quit
            if self.start.elapsed() > Duration::from_millis(1400) {
                let mut ppm = format!("P6 {w} {h} 255\n").into_bytes();
                ppm.extend(self.frame.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
                std::fs::write(path, ppm).unwrap();
                self.exit = true;
                return;
            }
        }
        let (buffer, canvas) = self
            .pool
            .create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888)
            .expect("create buffer");
        // shortcut: assumes a little-endian host, which Argb8888 in memory then matches.
        let bytes = unsafe { std::slice::from_raw_parts(self.frame.as_ptr() as *const u8, self.frame.len() * 4) };
        canvas[..bytes.len()].copy_from_slice(bytes);
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w as i32, h as i32);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        buffer.attach_to(surface).expect("buffer attach");
        self.layer.commit();
        self.frame_pending = true;
    }
}

impl CompositorHandler for Starfield {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Starfield {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Starfield {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        self.width = c.new_size.0.max(1);
        self.height = c.new_size.1.max(1);
        self.configured = true;
    }
}

impl ShmHandler for Starfield {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Starfield);

impl ProvidesRegistryState for Starfield {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(Starfield);

fn main() {
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    // `--on NAME --at X,Y`: the starfield for a monitor plugged in later, which the compositor starts and
    // which shows the part of the one sky that monitor covers.
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1));
    let at: (f64, f64) = arg("--at").and_then(|v| v.split_once(',')).and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))).unwrap_or((0.0, 0.0));
    let namespace = match arg("--on") { Some(name) => format!("seiza-starfield@{name}"), None => "seiza-starfield".into() };
    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Background, Some(namespace), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1); // fill the whole output, ignoring bars
    layer.set_size(0, 0);
    layer.commit();

    let cam = Cam { x: 0.0, y: 0.0, zoom: 1.0, screen: (1920.0, 1080.0) };
    let mut app = Starfield {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        width: 1,
        height: 1,
        configured: false,
        frame_pending: false,
        exit: false,
        target: cam,
        cur: cam,
        at,
        sky_stars: Vec::new(),
        sky_links: Vec::new(),
        overview: false,
        art: None,
        art_key: Vec::new(),
        art_a: 0.0,
        text: Text::new(load_font(), 17.0),
        shooters: Vec::new(),
        next_shot: 1.0,
        rng: 0x5E12A,
        shooting: seiza_common::config::flag("sky", "shooting", true),
        labels: seiza_common::config::flag("sky", "labels", true),
        threads: seiza_common::config::flag("sky", "threads", true),
        nebula_gain: std::env::var("SEIZA_STARFIELD_NEBULA").ok().or_else(|| seiza_common::config::get("sky", "nebula")).and_then(|v| v.parse().ok()).unwrap_or(0.4),
        bg: Vec::new(),
        spent: Duration::ZERO,
        frames: 0,
        bg_origin: (0.0, 0.0),
        bg_size: (0, 0),
        frame: Vec::new(),
        start: Instant::now(),
        last: Instant::now(),
    };

    let mut event_loop: EventLoop<Starfield> = EventLoop::try_new().unwrap();
    let handle = event_loop.handle();
    WaylandSource::new(conn, event_queue).insert(handle.clone()).unwrap();

    let (tx, rx) = channel::channel();
    thread::spawn(move || read_state(tx));
    handle
        .insert_source(rx, |ev, _, app: &mut Starfield| {
            if let channel::Event::Msg(m) = ev {
                app.target = Cam { x: m.cam.x + app.at.0, y: m.cam.y + app.at.1, ..m.cam }; // the camera, seen from this monitor
                app.overview = m.overview;
                app.sky_stars = m.stars;
                app.sky_links = m.links;
                // Look for a figure only when a star has actually moved.
                let key: Vec<(i64, i64)> = app.sky_stars.iter().map(|s| (s.x as i64, s.y as i64)).collect();
                if key != app.art_key {
                    let pos: Vec<(f64, f64)> = key.iter().map(|&(x, y)| (x as f64, y as f64)).collect();
                    app.art = art::find(&pos);
                    app.art_key = key;
                }
            }
        })
        .unwrap();
    handle
        .insert_source(Timer::from_duration(FRAME), move |_, _, app| {
            app.tick(&qh);
            TimeoutAction::ToDuration(FRAME)
        })
        .unwrap();

    let signal = event_loop.get_signal();
    event_loop
        .run(None, &mut app, |app| {
            if app.exit {
                signal.stop();
            }
        })
        .unwrap();
}
