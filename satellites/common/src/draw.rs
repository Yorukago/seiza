//! Software drawing shared by the satellites: an ARGB canvas with antialiased
//! shapes, and text. Pixels are premultiplied ARGB8888, as wl_shm expects.
use std::collections::HashMap;

use fontdue::{Font, FontSettings};


/// First usable sans font: whatever fontconfig says, else a few well-known paths.
pub fn load_font() -> Font {
    let from_fc = std::process::Command::new("fc-match").args(["-f", "%{file}", "sans"]).output().ok().and_then(|o| String::from_utf8(o.stdout).ok());
    let candidates = from_fc.into_iter().chain(
        ["/usr/share/fonts/noto/NotoSans-Regular.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf", "/usr/share/fonts/liberation/LiberationSans-Regular.ttf"].map(String::from),
    );
    for path in candidates {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(f) = Font::from_bytes(bytes, FontSettings::default()) {
                return f;
            }
        }
    }
    panic!("no usable font found (install any sans TTF, e.g. noto-fonts)");
}

pub type Rgba = [f32; 4];

/// Source-over onto a premultiplied ARGB pixel.
pub fn blend(px: &mut u32, c: Rgba, cover: f32) {
    let a = (c[3] * cover).clamp(0.0, 1.0);
    let ch = |shift: u32, v: f32| {
        let dst = ((*px >> shift) & 0xFF) as f32;
        (v * a * 255.0 + dst * (1.0 - a)).min(255.0) as u32
    };
    let da = ((*px >> 24) & 0xFF) as f32;
    let na = (a * 255.0 + da * (1.0 - a)).min(255.0) as u32;
    *px = na << 24 | ch(16, c[0]) << 16 | ch(8, c[1]) << 8 | ch(0, c[2]);
}

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
}

impl Canvas {
    /// A rounded rectangle with antialiased edges (radius r; r = h/2 makes a pill).
    pub fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        let (x0, x1) = ((x.floor().max(0.0)) as usize, ((x + w).ceil() as usize).min(self.w));
        let (y0, y1) = ((y.floor().max(0.0)) as usize, ((y + h).ceil() as usize).min(self.h));
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        for py in y0..y1 {
            for px in x0..x1 {
                // distance to the rounded box's edge, negative inside
                let (dx, dy) = ((px as f32 + 0.5 - cx).abs() - (w / 2.0 - r), (py as f32 + 0.5 - cy).abs() - (h / 2.0 - r));
                let d = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r;
                let cover = (0.5 - d).clamp(0.0, 1.0);
                if cover > 0.0 {
                    blend(&mut self.px[py * self.w + px], c, cover);
                }
            }
        }
    }
}

/// Cover (0..1) of a shape at a pixel, by 3x3 supersampling of an inside-test.
fn cover(px: usize, py: usize, inside: impl Fn(f32, f32) -> bool) -> f32 {
    let mut n = 0;
    for sy in 0..3 {
        for sx in 0..3 {
            n += inside(px as f32 + (sx as f32 + 0.5) / 3.0, py as f32 + (sy as f32 + 0.5) / 3.0) as u32;
        }
    }
    n as f32 / 9.0
}

impl Canvas {
    pub fn shape(&mut self, cx: f32, cy: f32, r: f32, c: Rgba, inside: impl Fn(f32, f32) -> bool) {
        for py in (cy - r - 1.0).max(0.0) as usize..((cy + r + 1.0) as usize).min(self.h) {
            for px in (cx - r - 1.0).max(0.0) as usize..((cx + r + 1.0) as usize).min(self.w) {
                let k = cover(px, py, &inside);
                if k > 0.0 {
                    blend(&mut self.px[py * self.w + px], c, k);
                }
            }
        }
    }

    /// A four-pointed sparkle (an astroid).
    pub fn sparkle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let e = 2.0 / 3.0;
        self.shape(cx, cy, r, c, |x, y| (x - cx).abs().powf(e) + (y - cy).abs().powf(e) <= r.powf(e));
    }

    /// A five-pointed star.
    pub fn star5(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let pts: Vec<(f32, f32)> = (0..10)
            .map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let rr = if i % 2 == 0 { r } else { r * 0.42 };
                (cx + rr * a.cos(), cy + rr * a.sin())
            })
            .collect();
        self.shape(cx, cy, r, c, |x, y| {
            let mut inside = false; // even-odd point in polygon
            for i in 0..pts.len() {
                let ((x1, y1), (x2, y2)) = (pts[i], pts[(i + 1) % pts.len()]);
                if (y1 > y) != (y2 > y) && x < x1 + (y - y1) / (y2 - y1) * (x2 - x1) {
                    inside = !inside;
                }
            }
            inside
        });
    }

    /// A crescent moon.
    pub fn crescent(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        self.shape(cx, cy, r, c, |x, y| (x - cx).hypot(y - cy) <= r && (x - cx - r * 0.55).hypot(y - cy + r * 0.2) > r * 0.8);
    }
}

/// Draw `src` onto `dst` scaled by `s` about `c`, shifted down by `dy`, at opacity `a`.
/// `area` limits the work to the box the scaled content can reach (x, y, w, h in dst).
pub fn composite(dst: &mut Canvas, src: &Canvas, c: (f32, f32), s: f32, dy: f32, a: f32, area: Option<(f32, f32, f32, f32)>) {
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

#[derive(Clone)]
pub struct Text {
    font: Font,
    px: f32,
    cache: HashMap<char, (fontdue::Metrics, Vec<u8>)>,
}

impl Text {
    pub fn new(font: Font, px: f32) -> Text {
        Text { font, px, cache: HashMap::new() }
    }

    pub fn glyph(&mut self, ch: char) -> &(fontdue::Metrics, Vec<u8>) {
        self.cache.entry(ch).or_insert_with(|| self.font.rasterize(ch, self.px))
    }

    pub fn width(&mut self, s: &str) -> f32 {
        s.chars().map(|c| self.glyph(c).0.advance_width).sum()
    }

    /// Draw `s` with its baseline at `base`; returns the x after the last glyph.
    pub fn draw(&mut self, cv: &mut Canvas, mut x: f32, base: f32, s: &str, c: Rgba) -> f32 {
        for ch in s.chars() {
            let (m, bitmap) = self.glyph(ch).clone();
            let (gx, gy) = (x.round() as i32 + m.xmin, base.round() as i32 - m.height as i32 - m.ymin);
            for row in 0..m.height {
                for col in 0..m.width {
                    let (px, py) = (gx + col as i32, gy + row as i32);
                    if px >= 0 && py >= 0 && (px as usize) < cv.w && (py as usize) < cv.h {
                        blend(&mut cv.px[py as usize * cv.w + px as usize], c, bitmap[row * m.width + col] as f32 / 255.0);
                    }
                }
            }
            x += m.advance_width;
        }
        x
    }
}


/// Simple monoline icons for the session menu and the bar. `r` is the icon's radius.
impl Canvas {
    fn ring(&mut self, cx: f32, cy: f32, r: f32, thick: f32, c: Rgba, skip: impl Fn(f32) -> bool) {
        self.shape(cx, cy, r + 1.0, c, |x, y| {
            let (dx, dy) = (x - cx, y - cy);
            let d = dx.hypot(dy);
            d <= r && d >= r - thick && !skip(dy.atan2(dx).to_degrees())
        });
    }

    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgba) {
        self.shape((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0).max(y1 - y0) / 2.0 + 1.0, c, |x, y| x >= x0 && x <= x1 && y >= y0 && y <= y1);
    }

    /// A ring open at the top, with a bar through the gap.
    pub fn icon_power(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let t = (r * 0.2).max(1.6);
        self.ring(cx, cy, r, t, c, |a| (-125.0..-55.0).contains(&a));
        self.rect(cx - t / 2.0, cy - r * 1.05, cx + t / 2.0, cy - r * 0.1, c);
    }

    /// A padlock.
    pub fn icon_lock(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let t = (r * 0.2).max(1.6);
        self.ring(cx, cy - r * 0.15, r * 0.55, t, c, |a| a > 0.0); // the shackle: the top half of a ring
        self.rect(cx - r * 0.55, cy - r * 0.15, cx - r * 0.55 + t, cy + r * 0.05, c);
        self.rect(cx + r * 0.55 - t, cy - r * 0.15, cx + r * 0.55, cy + r * 0.05, c);
        self.rect(cx - r * 0.8, cy + r * 0.05, cx + r * 0.8, cy + r * 0.95, c);
    }

    /// A circular arrow.
    pub fn icon_restart(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let t = (r * 0.2).max(1.6);
        self.ring(cx, cy, r, t, c, |a| (-70.0..-15.0).contains(&a));
        let (tx, ty) = (cx + r * 0.62, cy - r * 0.78); // arrow head at the gap's start
        self.shape(tx, ty, r * 0.6, c, |x, y| {
            let (dx, dy) = ((x - tx) / (r * 0.5), (y - ty) / (r * 0.5));
            dx > -0.9 && dx < 0.9 && dy > -0.9 && dy < 0.9 && dx - dy > -0.5 && dx + dy < 0.9 && dy > -0.9 + (dx + 0.9) * 0.2 - 0.2
        });
    }

    /// An arrow leaving a doorway.
    pub fn icon_logout(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let t = (r * 0.2).max(1.6);
        self.rect(cx - r * 0.9, cy - r * 0.9, cx - r * 0.9 + t, cy + r * 0.9, c); // door frame
        self.rect(cx - r * 0.9, cy - r * 0.9, cx - r * 0.1, cy - r * 0.9 + t, c);
        self.rect(cx - r * 0.9, cy + r * 0.9 - t, cx - r * 0.1, cy + r * 0.9, c);
        self.rect(cx - r * 0.35, cy - t / 2.0, cx + r * 0.9, cy + t / 2.0, c); // the arrow
        let tip = cx + r * 0.95;
        self.shape(cx + r * 0.6, cy, r * 0.6, c, |x, y| x <= tip && (y - cy).abs() <= (tip - x) * 0.9 && x >= cx + r * 0.3);
    }

    /// A chip with pins: "boot to firmware".
    pub fn icon_chip(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let t = (r * 0.2).max(1.6);
        let s = r * 0.6;
        self.rect(cx - s, cy - s, cx + s, cy - s + t, c);
        self.rect(cx - s, cy + s - t, cx + s, cy + s, c);
        self.rect(cx - s, cy - s, cx - s + t, cy + s, c);
        self.rect(cx + s - t, cy - s, cx + s, cy + s, c);
        for k in [-0.35f32, 0.35] {
            self.rect(cx + k * r - t / 2.0, cy - r, cx + k * r + t / 2.0, cy - s, c);
            self.rect(cx + k * r - t / 2.0, cy + s, cx + k * r + t / 2.0, cy + r, c);
            self.rect(cx - r, cy + k * r - t / 2.0, cx - s, cy + k * r + t / 2.0, c);
            self.rect(cx + s, cy + k * r - t / 2.0, cx + r, cy + k * r + t / 2.0, c);
        }
        self.rect(cx - s * 0.35, cy - s * 0.35, cx + s * 0.35, cy + s * 0.35, c);
    }
}
