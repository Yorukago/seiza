//! The help pages: content, layout and drawing, with no Wayland in it so `--dump` and the
//! tests can run it. The key bindings come from the file the compositor rewrites on every
//! (re)load of ~/.config/seiza/keys, so they are never stale; the other pages are written here.

use std::time::Instant;

use seiza_common::{
    config,
    draw::{composite, load_font, Canvas, Text},
    palette::{palette, rgba},
};

/// The card's size before the first draw; after that it follows the screen (see `fit`).
const W0: f32 = 1240.0;
const H0: f32 = 740.0;
const NAV_W: f32 = 190.0;
const ROW_H: f32 = 34.0;
const LINE_H: f32 = 20.0; // a wrapped line that is not the last of its row
const TOP: f32 = 112.0;
const KROW: f32 = 25.0; // a row of the keys page
const KHEAD: f32 = 34.0; // a group's heading
const KSKIP: f32 = 14.0; // between groups
const KGAP: f32 = 28.0; // between its two columns
const KDESC: f32 = 274.0; // where a description starts, from the column's left
const SLIDE: f32 = 90.0; // how far below its place the card starts, and ends when it closes
const GLIDE_SECS: f32 = 0.16; // the nav pill sliding to another page
const KMIN: f32 = 440.0; // the narrowest a keys column may get before the page goes down to one column

pub const PAGES: [&str; 6] = ["keys", "commands", "guide", "recipes", "options", "files"];

/// What a binding's command does and which group it is listed under, by the start of the command. First match wins.
const WHAT: &[(&str, &str, &str)] = &[
    ("seiza quit", "leave Seiza", "Session"),
    ("seiza pan", "pan the camera", "Sky"),
    ("seiza zoom-reset", "reset the zoom", "Sky"),
    ("seiza overview", "see every star", "Sky"),
    ("seiza new-star", "make a new star", "Sky"),
    ("seiza delete-star", "delete this star", "Sky"),
    ("seiza relaunch", "restart satellites", "Session"),
    ("seiza move-window", "move the window", "Windows"),
    ("seiza resize-window", "resize the window", "Windows"),
    ("seiza focus", "focus that way", "Windows"),
    ("seiza goto", "fly to star", "Sky"),
    ("seiza close-window", "close the window", "Windows"),
    ("seiza maximize", "maximize", "Windows"),
    ("seiza cycle-window", "next window", "Windows"),
    ("seiza-help", "this help", "Apps"),
    ("seiza tile", "tile this star", "Windows"),
    ("seiza switch", "window switcher", "Windows"),
    ("seiza-launcher --clip", "clipboard history", "Apps"),
    ("seiza-launcher", "launcher", "Apps"),
    ("seiza-shot area", "screenshot an area", "Capture"),
    ("seiza-shot screen", "screenshot the screen", "Capture"),
    ("seiza-shot record", "record the screen", "Capture"),
    ("wpctl set-volume -l", "volume up", "Media"),
    ("wpctl set-volume", "volume down", "Media"),
    ("wpctl set-mute", "mute", "Media"),
    ("brightnessctl set 5%+", "brighter", "Media"),
    ("brightnessctl set 5%-", "dimmer", "Media"),
    ("playerctl play-pause", "play or pause", "Media"),
    ("playerctl next", "next track", "Media"),
    ("playerctl previous", "previous track", "Media"),
    ("seiza-lock", "lock the screen", "Session"),
    ("seiza-settings", "settings", "Apps"),
    ("seiza-constellations", "edit constellation", "Apps"),
    ("seiza-carousel", "browse constellations", "Apps"),
    ("seiza-panel", "side panel", "Apps"),
    ("seiza-hh", "palettes", "Apps"),
    ("kitty", "terminal", "Apps"),
];

/// The order the groups are listed in; a command no row above knows goes under "Other".
const GROUPS: [&str; 7] = ["Sky", "Windows", "Apps", "Capture", "Media", "Session", "Other"];

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

/// One line of the keys page: a group heading, or a binding drawn as keycaps (the last is the key itself)
/// with what it does.
pub enum Item {
    Head(String),
    Bind(Vec<String>, String),
}

fn key_label(k: &str) -> String {
    match k {
        "Escape" => "Esc",
        "Return" => "Enter",
        "XF86AudioRaiseVolume" => "VolUp",
        "XF86AudioLowerVolume" => "VolDown",
        "XF86AudioMute" => "Mute",
        "XF86AudioPlay" => "Play",
        "XF86AudioNext" => "Next",
        "XF86AudioPrev" => "Prev",
        "XF86MonBrightnessUp" => "BrightUp",
        "XF86MonBrightnessDown" => "BrightDown",
        _ => k,
    }
    .into()
}

/// "Super+Shift+Left = seiza pan..." lines to the keys page: grouped, and bindings that do the same thing
/// with only the last key differing (the four arrows, 1 to 9) folded into one row.
pub fn key_items(text: &str) -> Vec<Item> {
    struct Row {
        group: &'static str,
        what: String,
        alone: String, // its description if nothing folds into it (a lone `goto 3` says which star)
        mods: Vec<String>,
        keys: Vec<String>,
    }
    let mut rows: Vec<Row> = vec![];
    for (combo, cmd) in text.lines().filter_map(|l| l.split_once(" = ")) {
        let (group, what) = WHAT.iter().find(|(p, ..)| cmd.starts_with(p)).map_or(("Other", cmd), |(_, w, g)| (*g, *w));
        let mut parts: Vec<&str> = combo.split('+').collect();
        let key = parts.pop().unwrap_or("").to_string();
        let mods: Vec<String> = parts.iter().map(|m| m.to_string()).collect();
        let extra = if cmd.starts_with("seiza goto ") { &cmd["seiza goto ".len()..] } else { "" };
        match rows.iter_mut().find(|r| r.group == group && r.what == what && r.mods == mods) {
            Some(r) => r.keys.push(key),
            None => rows.push(Row { group, what: what.to_string(), alone: format!("{what} {extra}").trim().to_string(), mods, keys: vec![key] }),
        }
    }
    let mut out = vec![];
    for g in GROUPS {
        let mut any = false;
        for r in rows.iter().filter(|r| r.group == g) {
            if !std::mem::replace(&mut any, true) {
                out.push(Item::Head(g.to_string()));
            }
            let last = if r.keys.len() == 1 {
                key_label(&r.keys[0])
            } else if r.keys.iter().all(|k| matches!(k.as_str(), "Left" | "Right" | "Up" | "Down")) {
                "<Direction>".into()
            } else if r.keys.iter().all(|k| k.len() == 1 && k.as_bytes()[0].is_ascii_digit()) {
                "<Number>".into()
            } else {
                r.keys.iter().map(|k| key_label(k)).collect::<Vec<_>>().join("/")
            };
            let caps = r.mods.iter().cloned().chain([last]).collect();
            out.push(Item::Bind(caps, if r.keys.len() == 1 { r.alone.clone() } else { r.what.clone() }));
        }
    }
    out
}

struct KeyPlan {
    col: Vec<usize>,  // the column of each group
    used: Vec<f32>,   // the height of each column
    col_w: f32,
}

pub struct Ui {
    pub page: usize,
    pub scroll: usize, // rows scrolled off the top
    hover: Option<usize>, // the nav entry under the pointer
    glide: Option<(f32, Instant)>, // the nav pill sliding: the entry it left, and when
    stale: bool,                   // what is in `content` is out of date
    content: Option<Canvas>,       // the last frame drawn at full visibility
    back: Option<Canvas>,          // the card's plate, which only changes with the screen size
    w: f32,            // the card's size: most of the screen
    h: f32,
    keys: Vec<Item>,
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
        let font = load_font();
        let mut ui = Ui { page: 0, scroll: 0, hover: None, glide: None, stale: true, content: None, back: None, w: W0, h: H0, keys: vec![], t22: Text::new(font.clone(), 24.0), t17: Text::new(font.clone(), 17.0), t13: Text::new(font, 13.0) };
        ui.reload_keys();
        ui
    }

    pub fn reload_keys(&mut self) {
        self.stale = true;
        self.keys = key_items(&std::fs::read_to_string(keys_path()).unwrap_or_default());
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
        let table: Vec<(String, String)> = match self.page {
            1 => COMMANDS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            2 => GUIDE.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            3 => RECIPES.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
            4 => option_rows(),
            _ => FILES.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        };
        let room = self.w - NAV_W - 72.0 - self.key_w() - 10.0;
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

    fn visible(&self) -> usize {
        ((self.h - TOP - 24.0) / ROW_H) as usize
    }

    /// (rows in the page, rows that fit); the keys page counts its tallest column in key rows.
    fn extent(&mut self) -> (usize, usize) {
        if self.page != 0 {
            return (self.rows().len(), self.visible());
        }
        let (plan, krow) = (self.key_plan(), self.krow());
        let tallest = plan.used.iter().cloned().fold(0.0, f32::max);
        ((tallest / krow).ceil() as usize, (self.keys_room() / krow) as usize)
    }

    pub fn scroll_by(&mut self, d: isize) {
        let (total, visible) = self.extent();
        let d = if self.page == 0 { d * 3 } else { d }; // a keys row is short
        self.scroll = (self.scroll as isize + d).clamp(0, total.saturating_sub(visible) as isize) as usize;
        self.stale = true;
    }

    pub fn go(&mut self, page: usize) {
        let page = page.min(PAGES.len() - 1);
        if page != self.page {
            self.glide = Some((self.nav_pos(), Instant::now()));
        }
        self.page = page;
        self.scroll = 0;
        self.stale = true;
    }

    /// Where the nav pill is, as a (fractional) entry number.
    fn nav_pos(&self) -> f32 {
        let Some((from, t)) = self.glide else { return self.page as f32 };
        let k = (t.elapsed().as_secs_f32() / GLIDE_SECS).min(1.0);
        from + (self.page as f32 - from) * (1.0 - (1.0 - k).powi(3)) // eases out
    }

    fn gliding(&self) -> bool {
        self.glide.is_some_and(|(_, t)| t.elapsed().as_secs_f32() < GLIDE_SECS)
    }

    /// True while a frame is still owed: the pill is moving, or what is shown is out of date.
    pub fn pending(&self) -> bool {
        self.stale
    }

    /// Size the card to the screen: about three quarters of it, so the desktop shows around it.
    fn fit(&mut self, w: f32, h: f32) {
        self.w = (w * 0.74).clamp(900.0, 1500.0).min(w - 40.0);
        self.h = (h * 0.80).clamp(600.0, 900.0).min(h - 40.0);
    }

    /// The card sits a little below the middle, under the bar.
    fn origin(&self, w: f32, h: f32) -> (f32, f32) {
        ((w - self.w) / 2.0, (h - self.h) / 2.0 + 18.0)
    }

    /// A click: on a nav entry it switches page. Returns true if it closed nothing but changed something.
    pub fn press(&mut self, x: f32, y: f32, w: f32, h: f32) -> bool {
        match self.nav_at(x, y, w, h) {
            Some(i) => {
                self.go(i);
                true
            }
            None => false,
        }
    }

    /// The nav entry under (x, y), if any.
    fn nav_at(&mut self, x: f32, y: f32, w: f32, h: f32) -> Option<usize> {
        self.fit(w, h);
        let (ox, oy) = self.origin(w, h);
        let (px, py) = (x - ox, y - oy);
        (0..PAGES.len()).find(|&i| {
            let ny = 112.0 + i as f32 * 44.0;
            px >= 14.0 && px < NAV_W - 14.0 && py >= ny && py < ny + 40.0
        })
    }

    /// The pointer moved to `at` (None: it left); true if the highlight changed and a redraw is due.
    pub fn hover(&mut self, at: Option<(f32, f32)>, w: f32, h: f32) -> bool {
        let now = at.and_then(|(x, y)| self.nav_at(x, y, w, h));
        let changed = std::mem::replace(&mut self.hover, now) != now;
        self.stale |= changed;
        changed
    }

    /// One keycap at (x, y), 22 tall; returns where the next thing starts.
    fn cap(&mut self, cv: &mut Canvas, x: f32, y: f32, label: &str) -> f32 {
        let p = palette();
        let w = self.t13.width(label) + 16.0;
        cv.round_rect(x, y, w, 22.0, 6.0, rgba(p.text, 0.45));
        cv.round_rect(x + 1.0, y + 1.0, w - 2.0, 20.0, 5.0, rgba(p.panel, 1.0));
        self.t13.draw(cv, x + 8.0, y + 16.0, label, rgba(p.star, 1.0));
        x + w
    }

    fn krow(&self) -> f32 {
        if self.h > 800.0 { 28.0 } else { KROW }
    }

    /// Height available to the keys page's columns.
    fn keys_room(&self) -> f32 {
        self.h - TOP - 36.0
    }

    /// How the keys page is dealt: as many columns as are wide enough (KMIN) and each group whole, the tallest
    /// first, always into the shortest column; within a column the groups keep their order.
    fn key_plan(&self) -> KeyPlan {
        let sizes: Vec<usize> = self.keys.iter().fold(vec![], |mut v, it| {
            match it {
                Item::Head(_) => v.push(0),
                _ => *v.last_mut().expect("a binding follows its heading") += 1,
            }
            v
        });
        let room = self.w - NAV_W - 72.0;
        let cols = if room >= 2.0 * KMIN + KGAP { 2 } else { 1 };
        let mut by_size: Vec<usize> = (0..sizes.len()).collect();
        by_size.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
        let (mut col, mut used) = (vec![0usize; sizes.len()], vec![0.0f32; cols]);
        for i in by_size {
            let c = (0..cols).min_by(|&a, &b| used[a].total_cmp(&used[b])).unwrap_or(0);
            col[i] = c;
            used[c] += KHEAD + sizes[i] as f32 * self.krow() + KSKIP;
        }
        KeyPlan { col, used, col_w: (room - KGAP * (cols - 1) as f32) / cols as f32 }
    }

    /// The keys page, scrolled by `self.scroll` key rows; whatever does not fit whole is left out.
    fn draw_keys(&mut self, cv: &mut Canvas, x0: f32, y0: f32) {
        let p = palette();
        let (plan, krow) = (self.key_plan(), self.krow());
        let keys = std::mem::take(&mut self.keys); // so the text can be drawn while these are borrowed
        let mut groups: Vec<(&str, Vec<&Item>)> = vec![]; // (heading, bindings)
        for it in &keys {
            match it {
                Item::Head(g) => groups.push((g, vec![])),
                b => groups.last_mut().expect("a binding follows its heading").1.push(b),
            }
        }
        let room = self.keys_room();
        let fits = |y: f32, h: f32| y >= y0 && y + h <= y0 + room;
        let mut y = vec![y0 - self.scroll as f32 * krow; plan.used.len()];
        for (i, (head, binds)) in groups.iter().enumerate() {
            let c = plan.col[i];
            let x = x0 + c as f32 * (plan.col_w + KGAP);
            if fits(y[c], KHEAD) {
                self.t17.draw(cv, x, y[c] + 18.0, head, rgba(p.star, 1.0));
                cv.round_rect(x, y[c] + 25.0, plan.col_w, 1.0, 0.0, rgba(p.text, 0.12));
            }
            y[c] += KHEAD;
            for b in binds {
                let Item::Bind(caps, what) = b else { continue };
                if fits(y[c], krow) {
                    let mut cx = x;
                    for (k, cap) in caps.iter().enumerate() {
                        if k > 0 {
                            self.t13.draw(cv, cx + 4.0, y[c] + 15.0, "+", rgba(p.text, 0.5));
                            cx += 18.0;
                        }
                        cx = self.cap(cv, cx, y[c], cap);
                    }
                    self.t13.draw(cv, x + KDESC, y[c] + 15.0, what, rgba(p.text, 0.85));
                }
                y[c] += krow;
            }
            y[c] += KSKIP;
        }
        drop(groups);
        self.keys = keys;
    }

    /// The frame at visibility `e` (0 = gone, 1 = fully there): the card rises from below while it fades in.
    pub fn render(&mut self, w: usize, h: usize, e: f32) -> Canvas {
        self.fit(w as f32, h as f32);
        let g = self.gliding();
        if self.stale || g || self.content.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            self.content = Some(self.draw_card(w, h));
            self.stale = g; // a glide that was still going when this was drawn owes one more frame
            if !g {
                self.glide = None;
            }
        }
        let content = self.content.as_ref().expect("drawn above");
        if e >= 1.0 {
            return content.clone();
        }
        let dy = ((1.0 - e) * SLIDE).round();
        let (ox, oy) = self.origin(w as f32, h as f32);
        let mut out = Canvas { w, h, px: vec![0; w * h] };
        composite(&mut out, content, (0.0, 0.0), 1.0, dy, e.max(0.0), Some((ox - 2.0, oy - 2.0, self.w + 4.0, self.h + 4.0 + dy)));
        out
    }

    fn draw_card(&mut self, w: usize, h: usize) -> Canvas {
        let p = palette();
        let (ox, oy) = self.origin(w as f32, h as f32);
        if self.back.as_ref().is_none_or(|b| b.w != w || b.h != h) {
            let mut b = Canvas { w, h, px: vec![0; w * h] };
            b.round_rect(ox - 1.0, oy - 1.0, self.w + 2.0, self.h + 2.0, 25.0, rgba(p.star, 0.3));
            b.round_rect(ox, oy, self.w, self.h, 24.0, rgba(p.panel, 1.0));
            b.round_rect(ox, oy, NAV_W, self.h, 24.0, rgba(p.sky, 0.45));
            b.round_rect(ox + NAV_W - 30.0, oy, 30.0, self.h, 0.0, rgba(p.panel, 1.0)); // square the right corners,
            b.round_rect(ox + NAV_W - 30.0, oy, 30.0, self.h, 0.0, rgba(p.sky, 0.45)); // keeping the tint
            b.round_rect(ox + NAV_W, oy + 20.0, 1.0, self.h - 40.0, 0.0, rgba(p.text, 0.08));
            self.back = Some(b);
        }
        let mut cv = self.back.clone().expect("made above");

        self.t13.draw(&mut cv, ox + 28.0, oy + 52.0, "help", rgba(p.text, 0.45));
        let pill = oy + 112.0 + self.nav_pos() * 44.0;
        cv.round_rect(ox + 14.0, pill, NAV_W - 28.0, 40.0, 14.0, rgba(p.dust, 0.9));
        cv.sparkle(ox + 36.0, pill + 20.0, 6.0, rgba(p.star, 1.0));
        for (i, name) in PAGES.iter().enumerate() {
            let y = oy + 112.0 + i as f32 * 44.0;
            if self.hover == Some(i) && i != self.page {
                cv.round_rect(ox + 14.0, y, NAV_W - 28.0, 40.0, 14.0, rgba(p.dust, 0.4));
            }
            self.t17.draw(&mut cv, ox + 56.0, y + 26.0, name, rgba(p.text, if i == self.page { 1.0 } else if self.hover == Some(i) { 0.85 } else { 0.6 }));
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

        if self.page == 0 {
            self.draw_keys(&mut cv, x, oy + TOP);
        } else {
            let rows = self.rows();
            let key_w = self.key_w();
            let mut y = oy + TOP;
            for (n, (left, right, h)) in rows.iter().enumerate().skip(self.scroll) {
                if y + h > oy + self.h - 30.0 {
                    break;
                }
                if rows.get(n + 1).is_none_or(|r| !r.0.is_empty()) {
                    cv.round_rect(x, y + h - 4.0, self.w - NAV_W - 72.0, 1.0, 0.0, rgba(p.text, 0.07)); // under the last line of a row
                }
                let base = y + if *h == ROW_H && !left.is_empty() { 20.0 } else { 14.0 }; // a one-line row sits in the middle; wrapped lines are packed
                self.t13.draw(&mut cv, x, base, left, rgba(p.star, if left.starts_with('[') { 1.0 } else { 0.85 }));
                self.t13.draw(&mut cv, x + key_w, base, right, rgba(p.text, 0.85));
                y += h;
            }
        }
        let (total, visible) = self.extent();
        if total > visible {
            let (track, frac) = (self.h - TOP - 24.0, visible as f32 / total as f32);
            let top = self.scroll as f32 / total as f32;
            cv.round_rect(ox + self.w - 14.0, oy + TOP, 4.0, track, 2.0, rgba(p.dust, 0.9));
            cv.round_rect(ox + self.w - 14.0, oy + TOP + track * top, 4.0, track * frac, 2.0, rgba(p.star, 0.9));
        }
        self.t13.draw(&mut cv, x, oy + self.h - 18.0, "Left/Right or click: page    Up/Down or wheel: scroll    Esc / q: close", rgba(p.text, 0.4));
        cv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_are_grouped_and_folded() {
        let text = "Super+Left = seiza pan -100 0\nSuper+Right = seiza pan 100 0\nSuper+3 = seiza goto 3\nSuper+t = my-terminal\nSuper+h = seiza-help\n";
        let items = key_items(text);
        let shown: Vec<String> = items
            .iter()
            .map(|i| match i {
                Item::Head(g) => format!("# {g}"),
                Item::Bind(k, w) => format!("{} = {w}", k.join("+")),
            })
            .collect();
        assert_eq!(shown, ["# Sky", "Super+<Direction> = pan the camera", "Super+3 = fly to star 3", "# Apps", "Super+h = this help", "# Other", "Super+t = my-terminal"]);
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
    fn the_keys_page_fits_its_columns_to_the_screen() {
        let bind = |n: usize| (0..n).map(|i| format!("Super+{}Alt = seiza pan {i} 0\n", i)).collect::<String>();
        let mut ui = Ui::new();
        ui.keys = key_items(&(bind(10) + "Super+h = seiza-help\nSuper+q = seiza close-window\nSuper+m = seiza maximize\nSuper+l = seiza-launcher\n"));
        ui.fit(1920.0, 1080.0);
        assert_eq!(ui.key_plan().used.len(), 2);
        ui.fit(1280.0, 720.0);
        let plan = ui.key_plan();
        assert_eq!(plan.used.len(), 1);
        assert!(plan.col_w >= KDESC, "a description needs room beside its keys");
        ui.go(0);
        let (total, visible) = ui.extent();
        ui.scroll_by(1000);
        assert_eq!(ui.scroll, total.saturating_sub(visible));
    }

    #[test]
    fn the_pointer_highlights_a_nav_entry_once() {
        let mut ui = Ui::new();
        ui.fit(1920.0, 1080.0);
        let (ox, oy) = ui.origin(1920.0, 1080.0);
        assert!(ui.hover(Some((ox + 60.0, oy + 112.0 + 44.0 + 10.0)), 1920.0, 1080.0));
        assert_eq!(ui.hover, Some(1));
        assert!(!ui.hover(Some((ox + 61.0, oy + 112.0 + 44.0 + 12.0)), 1920.0, 1080.0), "same entry, no redraw");
        assert!(ui.hover(None, 1920.0, 1080.0));
    }

    #[test]
    fn the_nav_pill_glides_then_settles() {
        let mut ui = Ui::new();
        ui.render(1280, 720, 1.0);
        assert!(!ui.pending(), "nothing moving, nothing owed");
        ui.go(2);
        assert!(ui.pending() && ui.nav_pos() < 1.0, "just left entry 0");
        ui.render(1280, 720, 1.0);
        assert!(ui.pending(), "still gliding: another frame is owed");
        std::thread::sleep(std::time::Duration::from_millis((GLIDE_SECS * 1000.0) as u64 + 40));
        ui.render(1280, 720, 1.0);
        assert!(!ui.pending() && ui.nav_pos() == 2.0, "the last frame lands on the entry");
    }

    #[test]
    fn scroll_stays_in_range() {
        let mut ui = Ui::new();
        ui.go(1);
        ui.scroll_by(1000);
        let max = ui.rows().len() - ui.visible(); // wrapped lines count, so the font changes it
        assert_eq!(ui.scroll, max);
        ui.scroll_by(-1000);
        assert_eq!(ui.scroll, 0);
    }
}
