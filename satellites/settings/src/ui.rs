//! The settings page: state, layout and drawing, with no Wayland in it so `--dump`
//! and the tests can run it. Every setting lives in one of Seiza's small config
//! files (see `seiza_common::config`); the programs that care read their own.

use std::collections::HashMap;

use seiza_common::{
    config,
    draw::{load_font, Canvas, Text},
    palette::{palette, rgba},
};

pub const W: f32 = 900.0;
pub const H: f32 = 640.0;
const NAV_W: f32 = 190.0;
const ROW_H: f32 = 78.0;

#[derive(Clone, Copy)]
pub enum Ctl {
    Slider(f32, f32, f32), // min, max, step
    Choice(&'static [&'static str]),
    Toggle,
    Path,
    Button(&'static str),
    Swatches, // the palette's colors, read-only
}

pub struct Row {
    pub id: &'static str, // "file/key"
    pub label: &'static str,
    pub hint: &'static str,
    pub ctl: Ctl,
    pub default: &'static str,
}

pub struct Page {
    pub name: &'static str,
    pub blurb: &'static str,
    pub rows: Vec<Row>,
}

pub fn pages() -> Vec<Page> {
    let row = |id, label, hint, ctl, default| Row { id, label, hint, ctl, default };
    let tog = |id, label, hint, default| row(id, label, hint, Ctl::Toggle, default);
    vec![
        Page { name: "look", blurb: "seiza reads a picture and borrows its colors.", rows: vec![
            row("look/swatches", "the sky now", "the colors every program draws with", Ctl::Swatches, ""),
            row("look/wallpaper", "colors from picture", "wallust reads it", Ctl::Path, ""),
            row("look/random", "surprise me", "one from the same folder", Ctl::Button("random"), ""),
            row("look/style", "palette style", "how boldly the picture colors the sky", Ctl::Choice(&["soft", "hard", "comp", "salient"]), "soft"),
            row("look/apply", "borrow the colors", "restarts the bar, the sky and the widgets", Ctl::Button("apply"), ""),
            tog("look/contrast_guard", "keep text readable", "swaps in cream text if a picture is too dim", "1"),
        ]},
        Page { name: "window", blurb: "the frame around every window.", rows: vec![
            row("frame/radius", "corner roundness", "in pixels", Ctl::Slider(0.0, 32.0, 1.0), "14"),
            row("frame/border", "frame thickness", "in pixels", Ctl::Slider(0.0, 8.0, 1.0), "2"),
            row("frame/glow", "glow", "the bloom behind a window", Ctl::Choice(&["off", "soft", "bright"]), "soft"),
        ]},
        Page { name: "bar", blurb: "what the bar says.", rows: vec![
            row("look/clock", "clock", "24 hours or 12", Ctl::Choice(&["24", "12"]), "24"),
            tog("bar/date", "date", "next to the clock", "1"),
            tog("bar/stats", "system stats", "cpu and ram", "1"),
            tog("bar/volume", "volume", "the speaker level", "1"),
            tog("bar/battery", "battery", "only where there is one", "1"),
        ]},
        Page { name: "stars", blurb: "the sky behind everything.", rows: vec![
            row("sky/nebula", "painted sky", "how much cloud the night shows", Ctl::Slider(0.0, 1.0, 0.05), "0.4"),
            tog("sky/shooting", "shooting stars", "now and then", "1"),
            tog("sky/labels", "star names", "in the overview", "1"),
            tog("sky/threads", "threads", "the lines joining stars in the overview", "1"),
        ]},
        Page { name: "motion", blurb: "seiza is never in a hurry.", rows: vec![
            row("motion/pan_secs", "pan duration", "how long a pan between altitudes takes", Ctl::Slider(0.1, 2.0, 0.05), "0.35"),
            row("motion/easing", "easing", "how a pan settles", Ctl::Choice(&["drift", "glide", "float"]), "drift"),
            tog("motion/reduce", "reduce motion", "pans become jumps", "0"),
        ]},
        Page { name: "yoru", blurb: "how the launcher and the menus behave.", rows: vec![
            tog("yoru/lines", "Yoru's lines", "the little quips on the lock screen and launcher", "1"),
            tog("yoru/confirm", "confirm power actions", "press twice to shut down or log out", "1"),
            row("launcher/style", "launcher look", "a list, or apps as stars", Ctl::Choice(&["list", "stars"]), "list"),
            row("launcher/search", "web search", "what a query that is no app asks", Ctl::Choice(&["duckduckgo", "google", "startpage", "brave"]), "duckduckgo"),
            row("launcher/terminal", "terminal", "runs apps that want one; kitty if empty", Ctl::Path, ""),
        ]},
        Page { name: "widgets", blurb: "what lives on the sky.", rows: vec![
            tog("widgets/clock", "clock", "the time, and the moon's phase", "1"),
            tog("widgets/calendar", "calendar", "this month", "1"),
            tog("widgets/system", "system", "cpu, ram and temperature", "1"),
        ]},
        Page { name: "keys", blurb: "keys are written in ~/.config/seiza/keys.", rows: vec![
            row("keys/help", "show all bindings", "asks seiza to list them", Ctl::Button("show"), ""),
            row("keys/reload", "reload keys", "after editing the file", Ctl::Button("reload"), ""),
        ]},
    ]
}

const WIDGET_NAMES: [&str; 3] = ["clock", "calendar", "system"];

/// The widgets file for the cards that are switched on, in the fixed order.
pub fn widgets_file(on: impl Fn(&str) -> bool) -> String {
    WIDGET_NAMES.iter().filter(|n| on(n)).map(|n| format!("{n}\n")).collect()
}

/// wallust's palette for the style chips.
pub fn style_palette(style: &str) -> &'static str {
    match style {
        "hard" => "harddark",
        "comp" => "darkcomp",
        "salient" => "saliencedark",
        _ => "dark",
    }
}

fn seed() -> usize {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize)
}

/// A picture from the folder `path` is in (or is), chosen by `seed`; ~/Pictures if `path` is empty.
pub fn random_picture(path: &str, seed: usize) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = if path.trim().is_empty() { format!("{home}/Pictures") } else { path.trim().replace('~', &home) };
    let p = std::path::Path::new(&path);
    let dir = if p.is_dir() { p } else { p.parent()? };
    let mut pics: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|f| f.extension().and_then(|x| x.to_str()).is_some_and(|x| ["jpg", "jpeg", "png", "webp"].contains(&x.to_lowercase().as_str())))
        .collect();
    pics.sort();
    pics.get(seed.checked_rem(pics.len())?).map(|f| f.to_string_lossy().into_owned())
}

/// A small preview of the picture at `path`, via ImageMagick if it is there; None if not.
fn thumbnail(path: &str) -> Option<(usize, usize, Vec<u32>)> {
    let path = path.trim().replace('~', &std::env::var("HOME").unwrap_or_default());
    let out = std::process::Command::new("magick").args([&format!("{path}[0]"), "-resize", "96x54^", "-gravity", "center", "-extent", "96x54", "ppm:-"]).output().ok()?;
    let nl = out.stdout.iter().enumerate().filter(|(_, b)| **b == b'\n').nth(2)?.0 + 1; // "P6\n96 54\n255\n"
    let (w, h) = (96, 54);
    let px = out.stdout.get(nl..nl + w * h * 3)?;
    Some((w, h, px.chunks(3).map(|c| 0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect()))
}

pub struct Ui {
    pages: Vec<Page>,
    pub page: usize,
    pub values: HashMap<&'static str, String>,
    pub focus: Option<&'static str>, // the text field being typed into
    pub msg: String,
    pub restart: Vec<&'static str>, // programs to restart when the page closes
    thumb: Option<(usize, usize, Vec<u32>)>,
    t22: Text,
    t17: Text,
    t13: Text,
}

/// What a click or key asks the outside world to do after the state changed.
pub enum Effect {
    None,
    ReloadConfig,
    ApplyColors,
    Run(&'static str),
}

impl Ui {
    pub fn new() -> Ui {
        let pages = pages();
        let mut values = HashMap::new();
        let widgets = std::fs::read_to_string(config::path("widgets")).ok();
        for r in pages.iter().flat_map(|p| &p.rows) {
            let v = match (r.id.split_once('/'), &widgets) {
                (Some(("widgets", name)), Some(text)) => if text.lines().any(|l| l.trim() == name) { "1" } else { "0" }.to_string(),
                (Some((file, key)), _) if file != "widgets" && matches!(r.ctl, Ctl::Toggle) => if config::flag(file, key, r.default == "1") { "1" } else { "0" }.to_string(),
                (Some((file, key)), _) if file != "widgets" => config::get(file, key).unwrap_or_else(|| r.default.to_string()),
                _ => r.default.to_string(),
            };
            values.insert(r.id, v);
        }
        let mut ui = Ui { pages, page: 0, values, focus: None, msg: "that's all for now..".into(), restart: vec![], thumb: None, t22: Text::new(load_font(), 24.0), t17: Text::new(load_font(), 17.0), t13: Text::new(load_font(), 13.0) };
        ui.load_thumb();
        ui
    }

    pub fn load_thumb(&mut self) {
        self.thumb = Some(self.values["look/wallpaper"].as_str()).filter(|p| !p.trim().is_empty()).and_then(thumbnail);
    }

    pub fn rows(&self) -> &[Row] {
        &self.pages[self.page].rows
    }

    pub fn page_names(&self) -> Vec<&'static str> {
        self.pages.iter().map(|p| p.name).collect()
    }

    fn origin(&self, w: f32, h: f32) -> (f32, f32) {
        ((w - W) / 2.0, (h - H) / 2.0)
    }

    /// The control's box for row `i`, relative to the panel.
    fn control(&self, i: usize) -> (f32, f32, f32, f32) {
        let (w, h) = match self.rows()[i].ctl {
            Ctl::Slider(..) => (240.0, 24.0),
            Ctl::Choice(opts) => (opts.len() as f32 * 84.0, 34.0),
            Ctl::Toggle => (50.0, 28.0),
            Ctl::Path => (330.0, 36.0),
            Ctl::Button(_) => (110.0, 36.0),
            Ctl::Swatches => (6.0 * 34.0, 28.0),
        };
        let x = W - 36.0 - w;
        let y = 112.0 + i as f32 * ROW_H + (ROW_H - h) / 2.0 - 6.0;
        (x, y, w, h)
    }

    pub fn set(&mut self, id: &'static str, val: String) -> Effect {
        self.values.insert(id, val.clone());
        let (file, key) = id.split_once('/').unwrap();
        let saved = match file {
            "widgets" => {
                let text = widgets_file(|n| self.values.get(format!("widgets/{n}").as_str()).is_some_and(|v| v == "1"));
                std::fs::create_dir_all(config::path("widgets").parent().unwrap()).and_then(|_| std::fs::write(config::path("widgets"), text))
            }
            "look" if matches!(key, "apply" | "random") => Ok(()),
            "keys" => Ok(()),
            _ => config::set(file, key, &val),
        };
        self.msg = match saved {
            Ok(()) => "saved..".into(),
            Err(e) => format!("couldn't save: {e}"),
        };
        let restart: &[&'static str] = match file {
            "sky" => &["seiza-starfield"],
            "widgets" => &["seiza-widgets"],
            "bar" => &["seiza-bar"],
            "look" if matches!(key, "clock" | "contrast_guard") => &["seiza-bar", "seiza-widgets", "seiza-starfield", "seiza-osd"],
            _ => &[],
        };
        for p in restart {
            if !self.restart.contains(p) {
                self.restart.push(p);
            }
        }
        match file {
            "look" if key == "random" => match random_picture(&self.values["look/wallpaper"], seed()) {
                Some(p) => {
                    self.values.insert("look/wallpaper", p);
                    Effect::ApplyColors
                }
                None => {
                    self.msg = "no pictures in that folder..".into();
                    Effect::None
                }
            },
            "motion" | "frame" => Effect::ReloadConfig,
            "keys" if key == "help" => Effect::Run("seiza help"),
            "keys" => Effect::ReloadConfig,
            "look" if key == "apply" => Effect::ApplyColors,
            _ => Effect::None,
        }
    }

    /// A press (or drag) at a screen point.
    pub fn press(&mut self, x: f32, y: f32, w: f32, h: f32, dragging: bool) -> Effect {
        let (ox, oy) = self.origin(w, h);
        let (px, py) = (x - ox, y - oy);
        if !dragging {
            self.focus = None;
        }
        if !dragging && px >= 0.0 && px < NAV_W {
            if let Some(i) = (0..self.pages.len()).find(|&i| py >= 112.0 + i as f32 * 44.0 && py < 112.0 + i as f32 * 44.0 + 40.0) {
                self.page = i;
            }
            return Effect::None;
        }
        for i in 0..self.rows().len() {
            let (cx, cy, cw, ch) = self.control(i);
            let id = self.rows()[i].id;
            let inside = px >= cx - 6.0 && px < cx + cw + 6.0 && py >= cy - 6.0 && py < cy + ch + 6.0;
            match self.rows()[i].ctl {
                Ctl::Slider(min, max, step) if inside || (dragging && self.focus == Some(id)) => {
                    self.focus = Some(id); // dragging stays on this slider even when the pointer leaves it
                    let t = ((px - cx) / cw).clamp(0.0, 1.0);
                    let v = (min + (max - min) * t) / step;
                    let v = v.round() * step;
                    return self.set(id, if step.fract() == 0.0 { format!("{v}") } else { format!("{v:.2}") });
                }
                Ctl::Choice(opts) if inside && !dragging => {
                    let n = (((px - cx) / cw) * opts.len() as f32) as usize;
                    return self.set(id, opts[n.min(opts.len() - 1)].to_string());
                }
                Ctl::Toggle if inside && !dragging => {
                    let on = self.values[id] == "1";
                    return self.set(id, if on { "0" } else { "1" }.into());
                }
                Ctl::Path if inside && !dragging => self.focus = Some(id),
                Ctl::Button(_) if inside && !dragging => return self.set(id, "1".into()),
                _ => {}
            }
        }
        Effect::None
    }

    pub fn release(&mut self) {
        if self.focus.is_some_and(|id| matches!(self.rows().iter().find(|r| r.id == id).map(|r| &r.ctl), Some(Ctl::Slider(..)))) {
            self.focus = None;
        }
    }

    /// Typing into the focused text field.
    pub fn type_text(&mut self, s: &str) {
        if let Some(id) = self.focus {
            self.values.get_mut(id).unwrap().push_str(s);
            let v = self.values[id].clone();
            self.set(id, v);
        }
    }

    pub fn backspace(&mut self) {
        if let Some(id) = self.focus {
            self.values.get_mut(id).unwrap().pop();
            let v = self.values[id].clone();
            self.set(id, v);
        }
    }

    pub fn render(&mut self, w: usize, h: usize) -> Canvas {
        let p = palette();
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let flat = {
            let a = 0.86;
            let ch = |v: f32| (v * a * 255.0) as u32;
            ((a * 255.0) as u32) << 24 | ch(p.sky[0]) << 16 | ch(p.sky[1]) << 8 | ch(p.sky[2])
        };
        cv.px.fill(flat);
        let (ox, oy) = self.origin(w as f32, h as f32);
        cv.round_rect(ox - 1.0, oy - 1.0, W + 2.0, H + 2.0, 25.0, rgba(p.star, 0.3));
        cv.round_rect(ox, oy, W, H, 24.0, rgba(p.panel, 1.0));
        cv.round_rect(ox, oy, NAV_W, H, 24.0, rgba(p.sky, 0.45));
        cv.round_rect(ox + NAV_W - 30.0, oy, 30.0, H, 0.0, rgba(p.panel, 1.0));
        cv.round_rect(ox + NAV_W, oy + 20.0, 1.0, H - 40.0, 0.0, rgba(p.text, 0.08));

        self.t13.draw(&mut cv, ox + 28.0, oy + 52.0, "settings", rgba(p.text, 0.45));
        for (i, name) in self.page_names().iter().enumerate() {
            let y = oy + 112.0 + i as f32 * 44.0;
            if i == self.page {
                cv.round_rect(ox + 14.0, y, NAV_W - 28.0, 40.0, 14.0, rgba(p.dust, 0.9));
                cv.sparkle(ox + 36.0, y + 20.0, 6.0, rgba(p.star, 1.0));
            }
            self.t17.draw(&mut cv, ox + 56.0, y + 26.0, name, rgba(p.text, if i == self.page { 1.0 } else { 0.6 }));
        }

        let page = &self.pages[self.page];
        let (name, blurb) = (page.name, page.blurb);
        self.t22.draw(&mut cv, ox + NAV_W + 36.0, oy + 58.0, name, rgba(p.text, 1.0));
        self.t13.draw(&mut cv, ox + NAV_W + 36.0, oy + 84.0, blurb, rgba(p.text, 0.55));

        for i in 0..self.rows().len() {
            let y = oy + 112.0 + i as f32 * ROW_H;
            cv.round_rect(ox + NAV_W + 36.0, y - 8.0, W - NAV_W - 72.0, 1.0, 0.0, rgba(p.text, 0.07));
            let (id, label, hint) = (self.rows()[i].id, self.rows()[i].label, self.rows()[i].hint);
            self.t17.draw(&mut cv, ox + NAV_W + 36.0, y + 28.0, label, rgba(p.text, 1.0));
            self.t13.draw(&mut cv, ox + NAV_W + 36.0, y + 50.0, hint, rgba(p.text, 0.5));
            let (cx, cy, cw, ch) = self.control(i);
            let (cx, cy) = (ox + cx, oy + cy);
            let val = self.values[id].clone();
            match self.rows()[i].ctl {
                Ctl::Slider(min, max, _) => {
                    let t = ((val.parse::<f32>().unwrap_or(min) - min) / (max - min)).clamp(0.0, 1.0);
                    cv.round_rect(cx, cy + ch / 2.0 - 3.0, cw, 6.0, 3.0, rgba(p.dust, 0.9));
                    cv.round_rect(cx, cy + ch / 2.0 - 3.0, (cw * t).max(6.0), 6.0, 3.0, rgba(p.star, 0.9));
                    cv.sparkle(cx + cw * t, cy + ch / 2.0, 10.0, rgba(p.star, 1.0));
                    let tw = self.t13.width(&val);
                    self.t13.draw(&mut cv, cx - 14.0 - tw, cy + ch / 2.0 + 5.0, &val, rgba(p.text, 0.7));
                }
                Ctl::Choice(opts) => {
                    cv.round_rect(cx, cy, cw, ch, ch / 2.0, rgba(p.sky, 0.6));
                    for (n, o) in opts.iter().enumerate() {
                        let ox2 = cx + n as f32 * 84.0;
                        let on = *o == val;
                        if on {
                            cv.round_rect(ox2 + 3.0, cy + 3.0, 78.0, ch - 6.0, (ch - 6.0) / 2.0, rgba(p.star, 0.95));
                        }
                        let tw = self.t13.width(o);
                        self.t13.draw(&mut cv, ox2 + 42.0 - tw / 2.0, cy + ch / 2.0 + 5.0, o, if on { rgba(p.sky, 1.0) } else { rgba(p.text, 0.8) });
                    }
                }
                Ctl::Toggle => {
                    let on = val == "1";
                    cv.round_rect(cx, cy, cw, ch, ch / 2.0, if on { rgba(p.star, 0.95) } else { rgba(p.dust, 0.9) });
                    let kx = if on { cx + cw - ch / 2.0 } else { cx + ch / 2.0 };
                    cv.round_rect(kx - 10.0, cy + ch / 2.0 - 10.0, 20.0, 20.0, 10.0, rgba(if on { p.sky } else { p.text }, 0.95));
                }
                Ctl::Path => {
                    let focused = self.focus == Some(id);
                    cv.round_rect(cx - 1.0, cy - 1.0, cw + 2.0, ch + 2.0, ch / 2.0 + 1.0, rgba(p.star, if focused { 0.7 } else { 0.2 }));
                    cv.round_rect(cx, cy, cw, ch, ch / 2.0, rgba(p.sky, 0.8));
                    if let (Some((tw, th, px)), true) = (&self.thumb, id == "look/wallpaper") {
                        let (tx, ty) = ((cx - 110.0) as usize, (cy + ch / 2.0) as usize - th / 2);
                        cv.round_rect(tx as f32 - 1.0, ty as f32 - 1.0, *tw as f32 + 2.0, *th as f32 + 2.0, 3.0, rgba(p.star, 0.3));
                        for y in 0..*th {
                            cv.px[(ty + y) * w + tx..(ty + y) * w + tx + tw].copy_from_slice(&px[y * tw..(y + 1) * tw]);
                        }
                    }
                    let shown = if val.is_empty() && !focused { (if id == "look/wallpaper" { "~/pictures/sky.jpg" } else { "kitty" }).to_string() } else { val.clone() };
                    // keep the tail of a long path in view
                    let tail: String = shown.chars().rev().scan(0.0, |acc, c| { *acc += self.t13.glyph(c).0.advance_width; (*acc < cw - 36.0).then_some(c) }).collect::<Vec<_>>().into_iter().rev().collect();
                    let end = self.t13.draw(&mut cv, cx + 18.0, cy + ch / 2.0 + 5.0, &tail, rgba(p.text, if val.is_empty() && !focused { 0.3 } else { 1.0 }));
                    if focused {
                        cv.round_rect(end + 1.0, cy + 9.0, 1.5, ch - 18.0, 0.0, rgba(p.text, 0.9));
                    }
                }
                Ctl::Swatches => {
                    for (n, c) in [p.sky, p.dust, p.mist, p.glow, p.star, p.text].into_iter().enumerate() {
                        let x = cx + n as f32 * 34.0;
                        cv.round_rect(x - 1.0, cy - 1.0, 30.0, ch + 2.0, 15.0, rgba(p.text, 0.25));
                        cv.round_rect(x, cy, 28.0, ch, 14.0, rgba(c, 1.0));
                    }
                }
                Ctl::Button(text) => {
                    cv.round_rect(cx, cy, cw, ch, ch / 2.0, rgba(p.star, 0.95));
                    let tw = self.t17.width(text);
                    self.t17.draw(&mut cv, cx + cw / 2.0 - tw / 2.0, cy + ch / 2.0 + 6.0, text, rgba(p.sky, 1.0));
                }
            }
        }
        self.t13.draw(&mut cv, ox + NAV_W + 36.0, oy + H - 28.0, &self.msg, rgba(p.text, 0.55));
        let ew = self.t13.width("esc to close");
        self.t13.draw(&mut cv, ox + W - 36.0 - ew, oy + H - 28.0, "esc to close", rgba(p.text, 0.35));
        cv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_config<R>(f: impl FnOnce() -> R) -> R {
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(()); // tests share one environment variable and one directory
        let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("seiza-settings-test-{}", std::process::id()));
        // SAFETY: the mutex above keeps the tests from touching the environment at once.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        let r = f();
        let _ = std::fs::remove_dir_all(dir);
        r
    }

    #[test]
    fn clicking_controls_saves_to_the_files_the_programs_read() {
        with_config(|| {
            let mut ui = Ui::new();
            assert_eq!(ui.values["motion/easing"], "drift");
            ui.page = 4; // motion
            let (w, h) = (1920.0, 1080.0);
            let (ox, oy) = ((w - W) / 2.0, (h - H) / 2.0);
            // the easing choice: click its second segment
            let (cx, cy, _, ch) = ui.control(1);
            assert!(matches!(ui.press(ox + cx + 84.0 + 40.0, oy + cy + ch / 2.0, w, h, false), Effect::ReloadConfig));
            assert_eq!(config::get("motion", "easing").as_deref(), Some("glide"));
            // the reduce toggle
            let (cx, cy, cw, ch) = ui.control(2);
            ui.press(ox + cx + cw / 2.0, oy + cy + ch / 2.0, w, h, false);
            assert_eq!(config::get("motion", "reduce").as_deref(), Some("1"));
            // a slider: click its far end
            let (cx, cy, cw, ch) = ui.control(0);
            ui.press(ox + cx + cw, oy + cy + ch / 2.0, w, h, false);
            assert_eq!(config::get("motion", "pan_secs").as_deref(), Some("2.00"));
            // the widgets toggles rewrite the whole file, and ask for a restart
            ui.page = 6;
            let (cx, cy, cw, ch) = ui.control(1); // calendar off
            ui.press(ox + cx + cw / 2.0, oy + cy + ch / 2.0, w, h, false);
            assert_eq!(std::fs::read_to_string(config::path("widgets")).unwrap(), "clock\nsystem\n");
            assert_eq!(ui.restart, ["seiza-widgets"]);
            // a fresh run reads it all back
            assert_eq!(Ui::new().values["widgets/calendar"], "0");
            assert_eq!(Ui::new().values["motion/easing"], "glide");
        });
    }

    #[test]
    fn frame_numbers_are_whole_toggles_read_on_off_and_keys_buttons_write_nothing() {
        with_config(|| {
            std::fs::create_dir_all(config::path("bar").parent().unwrap()).unwrap();
            std::fs::write(config::path("bar"), "date=off\n").unwrap();
            let mut ui = Ui::new();
            assert_eq!(ui.values["bar/date"], "0", "off in the file shows as off");
            assert!(matches!(ui.set("frame/radius", "20".into()), Effect::ReloadConfig));
            assert_eq!(config::get("frame", "radius").as_deref(), Some("20"));
            ui.page = 1;
            let (w, h) = (1920.0, 1080.0);
            let (cx, cy, cw, ch) = ui.control(0);
            ui.press((w - W) / 2.0 + cx + cw, (h - H) / 2.0 + cy + ch / 2.0, w, h, false);
            assert_eq!(config::get("frame", "radius").as_deref(), Some("32"), "no .00 for the C parser");
            assert!(matches!(ui.set("keys/help", "1".into()), Effect::Run("seiza help")));
            assert!(!config::path("keys").exists());
        });
    }

    #[test]
    fn random_picks_a_picture_next_to_the_current_one() {
        let dir = std::env::temp_dir().join(format!("seiza-pics-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(random_picture(dir.to_str().unwrap(), 0).is_none(), "an empty folder has none");
        for n in ["a.jpg", "b.PNG", "notes.txt"] {
            std::fs::write(dir.join(n), "").unwrap();
        }
        let from = dir.join("a.jpg");
        assert!(random_picture(from.to_str().unwrap(), 1).unwrap().ends_with("b.PNG"));
        assert!(random_picture(dir.to_str().unwrap(), 2).unwrap().ends_with("a.jpg"), "the seed wraps");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn typing_goes_to_the_focused_field_only() {
        with_config(|| {
            let mut ui = Ui::new();
            ui.type_text("x");
            assert_eq!(ui.values["look/wallpaper"], "");
            ui.focus = Some("look/wallpaper");
            ui.type_text("/a");
            ui.type_text("b");
            ui.backspace();
            assert_eq!(ui.values["look/wallpaper"], "/a");
        });
    }
}
