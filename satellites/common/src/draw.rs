//! Software drawing shared by the satellites: an ARGB canvas with antialiased
//! shapes, and text. Pixels are premultiplied ARGB8888, as wl_shm expects.
use std::collections::HashMap;

use fontdue::{Font, FontSettings};
use crate::config;


/// The font file fontconfig picks for `family`, but only if it really is that family (it answers with
/// some other font for a name it does not know).
fn font_file(family: &str) -> Option<String> {
    let out = std::process::Command::new("fc-match").args(["-f", "%{family}|%{file}", &format!("{family}:style=Regular")]).output().ok()?;
    let out = String::from_utf8(out.stdout).ok()?;
    let (found, file) = out.split_once('|')?;
    found.split(',').any(|f| f.trim().eq_ignore_ascii_case(family)).then(|| file.to_string())
}

/// First usable font: the one named by `look { font }` (Maple Mono NF unless set), else the system sans,
/// else a few well-known paths.
pub fn load_font() -> Font {
    let family = config::get("look", "font").unwrap_or_else(|| "Maple Mono NF".into());
    let from_fc = font_file(&family).or_else(|| {
        let o = std::process::Command::new("fc-match").args(["-f", "%{file}", "sans"]).output().ok()?;
        String::from_utf8(o.stdout).ok()
    });
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

/// A Japanese-capable font for the glyphs the main one lacks; loaded once, None if fontconfig has none.
fn cjk_font() -> Option<&'static Font> {
    static F: std::sync::OnceLock<Option<Font>> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let out = std::process::Command::new("fc-match").args(["-f", "%{file}", "sans:lang=ja"]).output().ok()?;
        Font::from_bytes(std::fs::read(String::from_utf8(out.stdout).ok()?).ok()?, FontSettings::default()).ok()
    })
    .as_ref()
}

/// Colour emoji: the bitmaps in Noto Color Emoji (or any CBDT/sbix emoji font fontconfig names for "emoji").
/// None if there is no such font; then emoji draw as the main font draws them, a box.
fn emoji_face() -> Option<&'static ttf_parser::Face<'static>> {
    static F: std::sync::OnceLock<Option<ttf_parser::Face<'static>>> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        let out = std::process::Command::new("fc-match").args(["-f", "%{file}", "Noto Color Emoji"]).output().ok()?;
        let bytes: &'static [u8] = Box::leak(std::fs::read(String::from_utf8(out.stdout).ok()?).ok()?.into_boxed_slice()); // lives for the whole run
        ttf_parser::Face::parse(bytes, 0).ok()
    })
    .as_ref()
}

/// One emoji, scaled to `px` tall, premultiplied ARGB.
#[derive(Clone)]
struct Sprite {
    w: usize,
    h: usize,
    px: Vec<u32>,
}

/// The picture of `ch` at `px` tall, if the emoji font has one. shortcut: single code points only; a flag,
/// a family or a skin tone draws as its parts. Upgrade by reading the font's GSUB ligatures.
fn emoji_sprite(ch: char, px: f32) -> Option<Sprite> {
    let face = emoji_face()?;
    let img = face.glyph_raster_image(face.glyph_index(ch)?, 136)?;
    if img.format != ttf_parser::RasterImageFormat::PNG {
        return None;
    }
    let mut dec = png::Decoder::new(std::io::Cursor::new(img.data));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let ch_n = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        _ => return None,
    };
    let (sw, sh) = (info.width as usize, info.height as usize);
    let scale = px * 1.1 / sh as f32; // a little taller than the text, as emoji usually are
    let (w, h) = (((sw as f32 * scale).round() as usize).max(1), ((sh as f32 * scale).round() as usize).max(1));
    let mut out = vec![0u32; w * h];
    for y in 0..h {
        for x in 0..w { // a box filter over the source pixels this one covers, on premultiplied colour
            let (x0, x1, y0, y1) = (x * sw / w, ((x + 1) * sw / w).max(x * sw / w + 1), y * sh / h, ((y + 1) * sh / h).max(y * sh / h + 1));
            let mut sum = [0u32; 4];
            for sy in y0..y1.min(sh) {
                for sx in x0..x1.min(sw) {
                    let i = (sy * sw + sx) * ch_n;
                    let a = if ch_n == 4 { buf[i + 3] as u32 } else { 255 };
                    sum[0] += a;
                    for c in 0..3 {
                        sum[c + 1] += buf[i + c] as u32 * a / 255;
                    }
                }
            }
            let n = ((y1.min(sh) - y0) * (x1.min(sw) - x0)).max(1) as u32;
            out[y * w + x] = sum[0] / n << 24 | sum[1] / n << 16 | sum[2] / n << 8 | sum[3] / n;
        }
    }
    Some(Sprite { w, h, px: out })
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

#[derive(Clone)]
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

/// Variation selectors and the zero-width joiner: they change an emoji, and draw nothing of their own.
fn is_joiner(c: char) -> bool {
    matches!(c, '\u{FE0E}' | '\u{FE0F}' | '\u{200D}')
}

#[derive(Clone)]
pub struct Text {
    font: Font,
    px: f32,
    cache: HashMap<char, (fontdue::Metrics, Vec<u8>)>,
    emoji: HashMap<char, Option<Sprite>>,
}

impl Text {
    pub fn new(font: Font, px: f32) -> Text {
        Text { font, px, cache: HashMap::new(), emoji: HashMap::new() }
    }

    pub fn glyph(&mut self, ch: char) -> &(fontdue::Metrics, Vec<u8>) {
        self.cache.entry(ch).or_insert_with(|| {
            // the CJK font is huge to parse, so only look at it for a glyph the main font lacks
            match (self.font.lookup_glyph_index(ch) == 0).then(cjk_font).flatten() {
                Some(f) if f.lookup_glyph_index(ch) != 0 => f.rasterize(ch, self.px),
                _ => self.font.rasterize(ch, self.px),
            }
        })
    }

    /// The colour picture for `ch`, if it is an emoji the main font has no glyph for.
    fn sprite(&mut self, ch: char) -> Option<&Sprite> {
        if (ch as u32) < 0x2190 || self.font.lookup_glyph_index(ch) != 0 {
            return None;
        }
        let px = self.px;
        self.emoji.entry(ch).or_insert_with(|| emoji_sprite(ch, px)).as_ref()
    }

    pub fn width(&mut self, s: &str) -> f32 {
        s.chars().map(|c| if is_joiner(c) { 0.0 } else if let Some(sp) = self.sprite(c) { sp.w as f32 + 2.0 } else { self.glyph(c).0.advance_width }).sum()
    }

    /// Draw `s` with its baseline at `base`; returns the x after the last glyph.
    pub fn draw(&mut self, cv: &mut Canvas, mut x: f32, base: f32, s: &str, c: Rgba) -> f32 {
        for ch in s.chars() {
            if is_joiner(ch) {
                continue;
            }
            if let Some(sp) = self.sprite(ch).cloned() {
                let (ox, oy) = (x.round() as i32 + 1, (base - self.px * 0.35 - sp.h as f32 / 2.0).round() as i32); // centred on the x-height
                for sy in 0..sp.h {
                    for sx in 0..sp.w {
                        let (px, py) = (ox + sx as i32, oy + sy as i32);
                        if px < 0 || py < 0 || px as usize >= cv.w || py as usize >= cv.h {
                            continue;
                        }
                        let p = sp.px[sy * sp.w + sx];
                        let d = &mut cv.px[py as usize * cv.w + px as usize];
                        let inv = 255 - (p >> 24);
                        let chan = |s: u32| ((p >> s & 255) + (*d >> s & 255) * inv / 255).min(255) << s;
                        *d = chan(24) | chan(16) | chan(8) | chan(0);
                    }
                }
                x += sp.w as f32 + 2.0;
                continue;
            }
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

    /// A sun: a disc with eight rays, turned by `angle` radians.
    pub fn icon_sun(&mut self, cx: f32, cy: f32, r: f32, angle: f32, c: Rgba) {
        let (w, sector) = ((r * 0.11).max(0.9), std::f32::consts::FRAC_PI_4);
        self.shape(cx, cy, r + 1.0, c, |x, y| {
            let (dx, dy) = (x - cx, y - cy);
            let d = dx.hypot(dy);
            if d <= r * 0.42 {
                return true;
            }
            let off = (dy.atan2(dx) - angle + sector / 2.0).rem_euclid(sector) - sector / 2.0; // from the nearest ray's axis
            d >= r * 0.64 && d <= r && d * off.sin().abs() <= w
        });
    }

    /// A speaker with three sound lines. `lines` is 0..=3 and fades them in one by one; `muted` swaps them for a cross.
    pub fn icon_speaker(&mut self, cx: f32, cy: f32, r: f32, lines: f32, muted: bool, c: Rgba) {
        let t = (r * 0.17).max(1.4);
        let (bx, tip) = (cx - r * 0.85, cx - r * 0.1); // the box starts at bx, the cone opens up to tip
        self.shape(cx, cy, r * 1.3, c, |x, y| {
            let dy = (y - cy).abs();
            let box_ = x >= bx && x <= bx + r * 0.35 && dy <= r * 0.3;
            let cone = x >= bx + r * 0.35 && x <= tip && dy <= r * 0.3 + (x - bx - r * 0.35) / (tip - bx - r * 0.35) * r * 0.45;
            box_ || cone
        });
        if muted {
            let (mx, a) = (cx + r * 0.55, r * 0.38);
            self.shape(mx, cy, a + t, c, |x, y| {
                let (dx, dy) = (x - mx, y - cy);
                dx.abs() <= a && dy.abs() <= a && ((dx - dy).abs() <= t * 0.7 || (dx + dy).abs() <= t * 0.7)
            });
            return;
        }
        for i in 0..3 {
            let a = ((lines - i as f32) * 2.0).clamp(0.0, 1.0);
            if a > 0.0 {
                self.ring(tip, cy, r * (0.55 + 0.45 * i as f32), t, [c[0], c[1], c[2], c[3] * a], |d| d.abs() > 42.0);
            }
        }
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

    /// A straight stroke from (x0, y0) to (x1, y1), `t` thick.
    fn seg(&mut self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), t: f32, c: Rgba) {
        let (cx, cy, r) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0).hypot(y1 - y0) / 2.0 + t);
        self.shape(cx, cy, r, c, |x, y| {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let k = (((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
            (x - x0 - k * dx).hypot(y - y0 - k * dy) <= t / 2.0
        });
    }

    /// Wi-Fi: three arcs and a dot. `off` draws only the dot.
    pub fn icon_wifi(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let t = (r * 0.2).max(1.6);
        let by = cy + r * 0.7;
        for k in [0.6f32, 1.05, 1.5] {
            self.ring(cx, by, r * k, t, c, |a| !(-135.0..-45.0).contains(&a));
        }
        self.shape(cx, by, t * 1.2, c, |x, y| (x - cx).hypot(y - by) <= t * 0.9);
    }

    /// A wired network: one box over two, joined by a bar.
    pub fn icon_ethernet(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let (t, b) = ((r * 0.2).max(1.6), r * 0.42);
        self.rect(cx - b, cy - r, cx + b, cy - r + b * 1.5, c);
        for dx in [-0.75f32, 0.75] {
            self.rect(cx + dx * r - b, cy + r - b * 1.5, cx + dx * r + b, cy + r, c);
        }
        self.rect(cx - t / 2.0, cy - r + b * 1.5, cx + t / 2.0, cy + r * 0.2, c);
        self.rect(cx - 0.75 * r, cy + r * 0.2 - t / 2.0, cx + 0.75 * r, cy + r * 0.2 + t / 2.0, c);
        for dx in [-0.75f32, 0.75] {
            self.rect(cx + dx * r - t / 2.0, cy + r * 0.2, cx + dx * r + t / 2.0, cy + r - b * 1.5, c);
        }
    }

    /// The Bluetooth rune.
    pub fn icon_bluetooth(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let t = (r * 0.2).max(1.6);
        let p = [(cx - r * 0.55, cy - r * 0.45), (cx + r * 0.55, cy + r * 0.45), (cx, cy + r), (cx, cy - r), (cx + r * 0.55, cy - r * 0.45), (cx - r * 0.55, cy + r * 0.45)];
        for w in p.windows(2) {
            self.seg(w[0], w[1], t, c);
        }
    }

    /// A microphone; `slash` crosses it out.
    pub fn icon_mic(&mut self, cx: f32, cy: f32, r: f32, slash: bool, c: Rgba) {
        let t = (r * 0.2).max(1.6);
        self.round_rect(cx - r * 0.3, cy - r, r * 0.6, r * 1.2, r * 0.3, c);
        self.ring(cx, cy - r * 0.1, r * 0.65, t, c, |a| a < 0.0);
        self.rect(cx - t / 2.0, cy + r * 0.55, cx + t / 2.0, cy + r * 0.9, c);
        self.rect(cx - r * 0.35, cy + r * 0.9 - t, cx + r * 0.35, cy + r * 0.9, c);
        if slash {
            self.seg((cx - r * 0.8, cy - r * 0.8), (cx + r * 0.8, cy + r * 0.8), t, c);
        }
    }

    /// A screenshot: the four corners of a frame.
    pub fn icon_frame(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let (t, a) = ((r * 0.2).max(1.6), r * 0.45);
        for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let (x, y) = (cx + sx * r, cy + sy * r);
            self.rect(x.min(x - sx * a), y.min(y - sy * t), x.max(x - sx * a), y.max(y - sy * t), c);
            self.rect(x.min(x - sx * t), y.min(y - sy * a), x.max(x - sx * t), y.max(y - sy * a), c);
        }
    }

    /// A record button: a ring with a disc inside.
    pub fn icon_record(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        self.ring(cx, cy, r, (r * 0.2).max(1.6), c, |_| false);
        self.shape(cx, cy, r, c, |x, y| (x - cx).hypot(y - cy) <= r * 0.5);
    }

    /// A clipboard with two lines of text.
    pub fn icon_clipboard(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let (t, w, h) = ((r * 0.2).max(1.6), r * 0.8, r * 0.95);
        self.rect(cx - w, cy - h + r * 0.15, cx - w + t, cy + h, c);
        self.rect(cx + w - t, cy - h + r * 0.15, cx + w, cy + h, c);
        self.rect(cx - w, cy + h - t, cx + w, cy + h, c);
        self.rect(cx - w, cy - h + r * 0.15, cx + w, cy - h + r * 0.15 + t, c);
        self.round_rect(cx - r * 0.4, cy - h - r * 0.05, r * 0.8, r * 0.4, r * 0.15, c); // the clip
        self.rect(cx - r * 0.4, cy - r * 0.05, cx + r * 0.4, cy - r * 0.05 + t, c);
        self.rect(cx - r * 0.4, cy + r * 0.35, cx + r * 0.2, cy + r * 0.35 + t, c);
    }

    /// Three stars joined by threads: the constellation view.
    pub fn icon_constellation(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        let t = (r * 0.16).max(1.4);
        let p = [(cx - r * 0.85, cy + r * 0.5), (cx - r * 0.1, cy - r * 0.35), (cx + r * 0.85, cy + r * 0.15), (cx + r * 0.3, cy + r * 0.85)];
        for w in p.windows(2) {
            self.seg(w[0], w[1], t, c);
        }
        for (x, y) in p {
            self.shape(x, y, r * 0.3, c, |px, py| (px - x).hypot(py - y) <= r * 0.26);
        }
    }

    /// A bell.
    pub fn icon_bell(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        self.shape(cx, cy, r * 1.1, c, |x, y| {
            let (dx, dy) = ((x - cx) / r, (y - cy) / r);
            let dome = dy < 0.1 && (dx / 0.6).powi(2) + ((dy + 0.1) / 0.75).powi(2) <= 1.0;
            let flare = (0.1..=0.55).contains(&dy) && dx.abs() <= 0.6 + (dy - 0.1) * 0.5;
            let clapper = (dx).hypot(dy - 0.78) <= 0.2;
            let knob = dx.hypot(dy + 0.9) <= 0.12;
            dome || flare || clapper || knob
        });
    }
}

#[cfg(test)]
mod emoji_tests {
    use super::*;

    #[test]
    fn an_emoji_has_width_and_colour_when_the_font_is_there() {
        if emoji_face().is_none() {
            return; // no emoji font on this machine: nothing to check
        }
        let mut t = Text::new(load_font(), 20.0);
        assert!(t.width("😀") > 10.0, "it takes room");
        assert_eq!(t.width("a\u{FE0F}"), t.width("a"), "a variation selector takes none");
        let mut cv = Canvas { w: 60, h: 40, px: vec![0; 60 * 40] };
        t.draw(&mut cv, 4.0, 28.0, "😀", [1.0, 1.0, 1.0, 1.0]);
        let yellow = cv.px.iter().any(|&p| p >> 24 > 200 && (p >> 16 & 255) > 200 && (p >> 8 & 255) > 150 && (p & 255) < 120);
        assert!(yellow, "the grin is drawn in colour, not in the text colour");
    }
}
