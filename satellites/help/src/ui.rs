//! The help pages: content, layout and drawing, with no Wayland in it so `--dump` and the
//! tests can run it. The key bindings come from the file the compositor rewrites on every
//! (re)load of ~/.config/seiza/keys, so they are never stale; the other pages are written here.

use seiza_common::{
    config,
    draw::{load_font, Canvas, Text},
    palette::{palette, rgba},
};

pub const W: f32 = 900.0;
pub const H: f32 = 640.0;
const NAV_W: f32 = 190.0;
const ROW_H: f32 = 34.0;
const LINE_H: f32 = 20.0; // a wrapped line that is not the last of its row
const TOP: f32 = 112.0;

pub const PAGES: [&str; 6] = ["keys", "commands", "guide", "recipes", "options", "files"];

/// What a binding's command does, by the start of the command. First match wins.
const WHAT: &[(&str, &str)] = &[
    ("seiza quit", "leave Seiza"),
    ("seiza pan", "pan the camera"),
    ("seiza zoom-reset", "reset the zoom"),
    ("seiza overview", "zoom out over the whole constellation"),
    ("seiza new-star", "make a new star"),
    ("seiza delete-star", "delete the current star (when empty)"),
    ("seiza relaunch", "restart the satellites"),
    ("seiza move-window", "move the window"),
    ("seiza resize-window", "resize the window"),
    ("seiza focus", "focus the window that way"),
    ("seiza goto", "fly to star"),
    ("seiza close-window", "close the window"),
    ("seiza maximize", "maximize the window"),
    ("seiza cycle-window", "next window"),
    ("seiza-help", "this help"),
    ("seiza tile", "tile or untile this star"),
    ("seiza switch", "switch windows (hold Super, Tab steps)"),
    ("seiza-launcher --clip", "clipboard history"),
    ("seiza-launcher", "open the launcher"),
    ("seiza-shot area", "screenshot of an area"),
    ("seiza-shot screen", "screenshot of the screen"),
    ("wpctl set-volume -l", "volume up"),
    ("wpctl set-volume", "volume down"),
    ("wpctl set-mute", "mute"),
    ("brightnessctl set 5%+", "brighter"),
    ("brightnessctl set 5%-", "dimmer"),
    ("playerctl play-pause", "play or pause"),
    ("playerctl next", "next track"),
    ("playerctl previous", "previous track"),
    ("seiza-lock", "lock the screen"),
    ("seiza-settings", "open settings"),
    ("seiza-constellations", "edit the constellation"),
    ("seiza-carousel", "browse constellations"),
];

const COMMANDS: &[(&str, &str)] = &[
    ("seiza overview", "zoom out over every star"),
    ("seiza pan DX DY", "move the camera by that many pixels"),
    ("seiza zoom-reset", "back to full size"),
    ("seiza goto N", "fly to star number N"),
    ("seiza new-star", "make a star next to the current one"),
    ("seiza delete-star [N]", "delete star N (default: current); it must be empty"),
    ("seiza rename-star N NAME", "give star N a name"),
    ("seiza add-star X Y", "a star centered there"),
    ("seiza move-star N X Y", "move a star"),
    ("seiza link A B", "toggle a thread between two stars"),
    ("seiza figure NAME|none", "lay the stars out on a template"),
    ("seiza save NAME / load NAME", "keep or open a constellation"),
    ("seiza new-constellation NAME", "a fresh one with a single star"),
    ("seiza move-window DX DY", "move the focused window"),
    ("seiza resize-window DW DH", "grow or shrink it"),
    ("seiza focus DX DY", "focus the nearest window that way"),
    ("seiza maximize / close-window", "the focused window"),
    ("seiza tile", "tile or untile this star (remembered)"),
    ("seiza switch", "the window switcher (Super+Tab)"),
    ("seiza cycle-window", "focus the next window"),
    ("seiza reload-config", "re-read keys, motion and the frame"),
    ("seiza relaunch", "restart the satellites"),
    ("seiza quit", "leave Seiza"),
    ("anything else", "runs as a shell command"),
];

const GUIDE: &[(&str, &str)] = &[
    ("star", "a place on the big canvas. Windows live in a star."),
    ("constellation", "a set of stars you can save and come back to."),
    ("overview", "zoom out to see every star; click one to fly in."),
    ("thread", "a line you draw between two stars, just for you."),
    ("satellite", "a small program Seiza starts: bar, sky, launcher..."),
    ("launcher", "type to find an app; Tab flips list and stars."),
    ("quick answers", "launcher: 2*(3+4), 5 km to mi, :moon. Enter copies."),
    ("tiling", "a tiled star arranges its windows; Super+t flips it."),
    ("rules", "rule in seiza.kdl: send an app to a star, size it."),
    ("cards", "drag to pin a card to a star; right-click moves it on."),
    ("settings", "colors, frame, bar, sky and motion; most apply at once."),
    ("lock", "the lock screen; it asks for your normal password."),
    ("every window", "can be moved and resized from the keyboard too."),
];

/// Copy these into ~/.config/seiza/seiza.kdl.
const RECIPES: &[(&str, &str)] = &[
    ("hide the bar", "bar { autohide on }  The bar becomes a thin strip at the top edge and grows when the pointer touches it."),
    ("tile every star", "layout { tile on; max 3; gap 10 }  The first 3 windows of a star are laid out for you. Super+t flips one star."),
    ("send an app to a star", "rule app=\"steam\" star=\"games\" mode=\"maximize\"  The first rule that matches wins. seiza debug-windows shows app ids."),
    ("size a window", "rule app=\"pavucontrol\" w=700 h=500  Give both w and h. title=\"part of the title\" matches by title instead."),
    ("see the weather", "weather { place \"Lisbon\" }, then turn on the weather card in settings, widgets. Without a place it guesses from your address."),
    ("see what is coming up", "calendar { source \"~/cal.ics\" }, then turn on the agenda card. A calendar's private address works too."),
    ("change the wallpaper by itself", "look { slideshow 30 }  Every 30 minutes the next picture in the wallpaper's folder recolors the sky. 0 is off."),
    ("lock when you leave", "idle { dim 120; lock 300; off 600 }  Seconds without input. 0 is never. Anything watching video keeps it awake."),
    ("bind a key", "binds { Super+Return { spawn \"kitty\" } }  Several actions: Super+g { goto 1; spawn \"kitty\"; }"),
    ("name a few actions", "def my-thing { goto 1; spawn \"notify-send hi\"; }  then use my-thing in a bind."),
    ("choose what starts", "autostart { spawn \"seiza-bar\"; spawn \"seiza-starfield\" }  Leave it out to get the usual ones. An empty block starts nothing."),
    ("check your file", "seiza-conf check  names mistakes by line. Saving the file reloads it by itself."),
];

const FILES: &[(&str, &str)] = &[
    ("seiza.kdl", "most settings in one file; seiza-conf docs lists them."),
    ("keys", "Mods+Key = command. Yours add to the defaults. Saved = shown here."),
    ("motion", "how the camera glides: pan_secs, easing, reduce."),
    ("palette", "the sky's colors (role=#hex). seiza-wallpaper writes it."),
    ("frame", "window corner radius, border and glow."),
    ("sky", "the starfield: nebula and friends."),
    ("look", "clock format, wallpaper, palette style, slideshow minutes."),
    ("widgets", "which cards show; weather and agenda are opt-in."),
    ("launcher", "style, search and terminal."),
    ("autostart", "programs to start with Seiza, one per line."),
    ("figures/NAME.fig", "your own constellation templates."),
    ("*.const", "saved constellations."),
];

/// "Super+Shift+Left = seiza pan..." lines to (keys, what it does).
pub fn parse_bindings(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|l| l.split_once(" = "))
        .map(|(k, cmd)| {
            let what = WHAT.iter().find(|(p, _)| cmd.starts_with(p)).map_or(cmd, |(_, w)| w);
            // Numbered goto/focus/move say which; the table's wording says what.
            let extra = if cmd.starts_with("seiza goto ") { format!(" {}", &cmd["seiza goto ".len()..]) } else { String::new() };
            (k.to_string(), format!("{what}{extra}"))
        })
        .collect()
}

pub struct Ui {
    pub page: usize,
    pub scroll: usize, // rows scrolled off the top
    keys: Vec<(String, String)>,
    t22: Text,
    t17: Text,
    t13: Text,
}

/// The options table as help rows: a heading per section, then each option with its range and default.
fn option_rows() -> Vec<(String, String)> {
    use seiza_common::conf::{Kind, OPTS, SECTIONS};
    let mut rows = vec![];
    for (sec, what) in SECTIONS {
        rows.push((format!("[{sec}]"), what.to_string()));
        for o in OPTS.iter().filter(|o| o.section == *sec) {
            let hint = match &o.kind {
                Kind::Flag => "on or off".to_string(),
                Kind::Num(lo, hi) => format!("{lo} to {hi}"),
                Kind::Pick(list) => list.join(", "),
                Kind::Text => "text".to_string(),
            };
            let default = if o.default.is_empty() { "empty".to_string() } else { o.default.to_string() };
            rows.push((format!("  {}", o.key), format!("{}  ({hint}; default {default})", o.doc)));
        }
    }
    rows
}

pub fn keys_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("seiza-keys")
}

impl Ui {
    pub fn new() -> Ui {
        let mut ui = Ui { page: 0, scroll: 0, keys: vec![], t22: Text::new(load_font(), 24.0), t17: Text::new(load_font(), 17.0), t13: Text::new(load_font(), 13.0) };
        ui.reload_keys();
        ui
    }

    pub fn reload_keys(&mut self) {
        self.keys = parse_bindings(&std::fs::read_to_string(keys_path()).unwrap_or_default());
    }

    fn key_w(&self) -> f32 {
        match self.page {
            0 => 230.0,
            3 | 4 => 190.0,
            _ => 250.0,
        }
    }

    /// (left, right, height) lines of the page. A long right side is wrapped onto lines with an empty left;
    /// the last line of a group has room under it, so a one-line group is ROW_H tall as it always was.
    fn rows(&mut self) -> Vec<(String, String, f32)> {
        if self.page == 0 {
            return self.keys.iter().map(|(a, b)| (a.clone(), b.clone(), ROW_H)).collect();
        }
        let table: Vec<(String, String)> = match self.page {
            1 => COMMANDS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            2 => GUIDE.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            3 => RECIPES.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            4 => option_rows(),
            _ => FILES.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        };
        let room = W - NAV_W - 72.0 - self.key_w() - 10.0;
        let mut out = vec![];
        for (left, right) in table {
            let mut lines: Vec<String> = vec![];
            for word in right.split(' ') {
                match lines.last_mut() {
                    Some(l) if self.t13.width(&format!("{l} {word}")) <= room => {
                        l.push(' ');
                        l.push_str(word);
                    }
                    _ => lines.push(word.to_string()),
                }
            }
            let n = lines.len();
            for (i, line) in lines.into_iter().enumerate() {
                out.push((if i == 0 { left.clone() } else { String::new() }, line, if i + 1 == n { ROW_H } else { LINE_H }));
            }
        }
        out
    }

    fn visible() -> usize {
        ((H - TOP - 24.0) / ROW_H) as usize
    }

    pub fn scroll_by(&mut self, d: isize) {
        let max = self.rows().len().saturating_sub(Self::visible());
        self.scroll = (self.scroll as isize + d).clamp(0, max as isize) as usize;
    }

    pub fn go(&mut self, page: usize) {
        self.page = page.min(PAGES.len() - 1);
        self.scroll = 0;
    }

    fn origin(w: f32, h: f32) -> (f32, f32) {
        ((w - W) / 2.0, (h - H) / 2.0)
    }

    /// A click: on a nav entry it switches page. Returns true if it closed nothing but changed something.
    pub fn press(&mut self, x: f32, y: f32, w: f32, h: f32) -> bool {
        let (ox, oy) = Self::origin(w, h);
        let (px, py) = (x - ox, y - oy);
        for i in 0..PAGES.len() {
            let ny = 112.0 + i as f32 * 44.0;
            if px >= 14.0 && px < NAV_W - 14.0 && py >= ny && py < ny + 40.0 {
                self.go(i);
                return true;
            }
        }
        false
    }

    pub fn render(&mut self, w: usize, h: usize) -> Canvas {
        let p = palette();
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let a = 0.86;
        let ch = |v: f32| (v * a * 255.0) as u32;
        cv.px.fill(((a * 255.0) as u32) << 24 | ch(p.sky[0]) << 16 | ch(p.sky[1]) << 8 | ch(p.sky[2]));
        let (ox, oy) = Self::origin(w as f32, h as f32);
        cv.round_rect(ox - 1.0, oy - 1.0, W + 2.0, H + 2.0, 25.0, rgba(p.star, 0.3));
        cv.round_rect(ox, oy, W, H, 24.0, rgba(p.panel, 1.0));
        cv.round_rect(ox, oy, NAV_W, H, 24.0, rgba(p.sky, 0.45));
        cv.round_rect(ox + NAV_W - 30.0, oy, 30.0, H, 0.0, rgba(p.panel, 1.0));
        cv.round_rect(ox + NAV_W, oy + 20.0, 1.0, H - 40.0, 0.0, rgba(p.text, 0.08));

        self.t13.draw(&mut cv, ox + 28.0, oy + 52.0, "help", rgba(p.text, 0.45));
        for (i, name) in PAGES.iter().enumerate() {
            let y = oy + 112.0 + i as f32 * 44.0;
            if i == self.page {
                cv.round_rect(ox + 14.0, y, NAV_W - 28.0, 40.0, 14.0, rgba(p.dust, 0.9));
                cv.sparkle(ox + 36.0, y + 20.0, 6.0, rgba(p.star, 1.0));
            }
            self.t17.draw(&mut cv, ox + 56.0, y + 26.0, name, rgba(p.text, if i == self.page { 1.0 } else { 0.6 }));
        }

        let blurb = match self.page {
            0 => format!("live from {}: edit it and this updates.", config::path("keys").display()),
            1 => "type these in a terminal, or bind them to a key.".into(),
            2 => "the words Seiza uses.".into(),
            3 => format!("copy a line into {}.", config::path("seiza.kdl").display()),
            4 => "every setting seiza.kdl takes; seiza-conf docs prints the same.".into(),
            _ => format!("in {}", config::path("").display()),
        };
        let x = ox + NAV_W + 36.0;
        self.t22.draw(&mut cv, x, oy + 58.0, PAGES[self.page], rgba(p.text, 1.0));
        self.t13.draw(&mut cv, x, oy + 84.0, &blurb, rgba(p.text, 0.55));

        let rows = self.rows();
        let key_w = self.key_w();
        let mut y = oy + TOP;
        for (n, (left, right, h)) in rows.iter().enumerate().skip(self.scroll) {
            if y + h > oy + H - 30.0 {
                break;
            }
            if rows.get(n + 1).is_none_or(|r| !r.0.is_empty()) {
                cv.round_rect(x, y + h - 4.0, W - NAV_W - 72.0, 1.0, 0.0, rgba(p.text, 0.07)); // under the last line of a row
            }
            let base = y + if *h == ROW_H && !left.is_empty() { 20.0 } else { 14.0 }; // a one-line row sits in the middle; wrapped lines are packed
            if self.page == 0 {
                let tw = self.t13.width(left);
                cv.round_rect(x, y + 3.0, tw + 20.0, 24.0, 12.0, rgba(p.dust, 0.9));
                self.t13.draw(&mut cv, x + 10.0, base, left, rgba(p.star, 1.0));
            } else {
                self.t13.draw(&mut cv, x, base, left, rgba(p.star, if left.starts_with('[') { 1.0 } else { 0.85 }));
            }
            self.t13.draw(&mut cv, x + key_w, base, right, rgba(p.text, 0.85));
            y += h;
        }
        if rows.len() > Self::visible() {
            let (track, frac) = (H - TOP - 24.0, Self::visible() as f32 / rows.len() as f32);
            let top = self.scroll as f32 / rows.len() as f32;
            cv.round_rect(ox + W - 14.0, oy + TOP, 4.0, track, 2.0, rgba(p.dust, 0.9));
            cv.round_rect(ox + W - 14.0, oy + TOP + track * top, 4.0, track * frac, 2.0, rgba(p.star, 0.9));
        }
        self.t13.draw(&mut cv, x, oy + H - 18.0, "Left/Right or click: page    Up/Down or wheel: scroll    Esc / q: close", rgba(p.text, 0.4));
        cv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_get_a_description() {
        let b = parse_bindings("Super+h = seiza-help\nSuper+3 = seiza goto 3\nSuper+t = my-terminal\nnot a binding\n");
        assert_eq!(b, [("Super+h".into(), "this help".into()), ("Super+3".into(), "fly to star 3".into()), ("Super+t".into(), "my-terminal".into())]);
    }

    #[test]
    fn the_options_page_lists_every_setting_with_its_default() {
        let rows = option_rows();
        for key in ["autohide", "slideshow", "place", "source", "tile"] {
            assert!(rows.iter().any(|(l, _)| l.trim() == key), "{key} missing");
        }
        assert!(rows.iter().any(|(l, r)| l.trim() == "radius" && r.contains("default 14")));
        let mut ui = Ui::new();
        ui.go(4);
        assert!(ui.rows().iter().all(|(_, _, h)| *h > 0.0));
    }

    #[test]
    fn scroll_stays_in_range() {
        let mut ui = Ui::new();
        ui.go(1);
        ui.scroll_by(1000);
        assert_eq!(ui.scroll, COMMANDS.len() - Ui::visible());
        ui.scroll_by(-1000);
        assert_eq!(ui.scroll, 0);
    }
}
