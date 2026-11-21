//! The help pages: content, layout and drawing, with no Wayland in it so `--dump` and the
//! tests can run it. The key bindings come from the file the compositor rewrites on every
//! (re)load of ~/.config/seiza/keys, so they are never stale; the other pages are written here.

use seiza_common::{
    config,
    draw::{load_font, Canvas, Text},
    kit::{self, Thread, PILL_W},
    palette::{palette, rgba},
};

/// The card's size before the first draw; after that it follows the screen (see `fit`).
const W0: f32 = 1240.0;
const H0: f32 = 740.0;
const PAD: f32 = 30.0; // between the card's edge and its content
const TOP: f32 = 168.0; // where a page's body starts, from the card's top
const FOOT: f32 = 60.0; // the strip under the body
const GAP: f32 = 18.0; // between cards
const MINCOL: f32 = 320.0; // the narrowest a column of cards may get
const ROW1: f32 = 43.0; // a keys row with its keycaps beside the words
const ROW2: f32 = 68.0; // ...and with them under the words, when they do not fit beside
const CAP_SEP: f32 = 18.0; // between two keycaps (the "+")

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

type Cards = &'static [(&'static str, &'static [(&'static str, &'static str)])];

const COMMANDS: Cards = &[
    ("sky", &[
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
    ]),
    ("constellations", &[
        ("seiza save NAME / load NAME", "keep or open a constellation"),
        ("seiza new-constellation NAME", "a fresh one with a single star"),
    ]),
    ("windows", &[
        ("seiza move-window DX DY", "move the focused window"),
        ("seiza resize-window DW DH", "grow or shrink it"),
        ("seiza focus DX DY", "focus the nearest window that way"),
        ("seiza maximize / close-window", "the focused window"),
        ("seiza tile", "tile or untile this star (remembered)"),
        ("seiza switch", "the window switcher (Super+Tab)"),
        ("seiza cycle-window", "focus the next window"),
    ]),
    ("session", &[
        ("seiza reload-config", "re-read keys, motion and the frame"),
        ("seiza relaunch", "restart the satellites"),
        ("seiza quit", "leave Seiza"),
        ("anything else", "runs as a shell command"),
    ]),
];

const GUIDE: Cards = &[
    ("the sky", &[
        ("star", "a place on the big canvas. Windows live in a star."),
        ("constellation", "a set of stars you can save and come back to."),
        ("overview", "zoom out to see every star; click one to fly in."),
        ("thread", "a line you draw between two stars, just for you."),
    ]),
    ("programs", &[
        ("satellite", "a small program Seiza starts: bar, sky, launcher..."),
        ("launcher", "type to find an app; Tab flips list and stars."),
        ("quick answers", "launcher: 2*(3+4), 5 km to mi, :moon. Enter copies."),
        ("settings", "colors, frame, bar, sky and motion; most apply at once."),
        ("lock", "the lock screen; it asks for your normal password."),
    ]),
    ("windows and cards", &[
        ("tiling", "a tiled star arranges its windows; Super+t flips it."),
        ("rules", "rule in seiza.kdl: send an app to a star, size it."),
        ("cards", "drag to pin a card to a star; right-click moves it on."),
        ("every window", "can be moved and resized from the keyboard too."),
    ]),
];

/// Copy these into ~/.config/seiza/seiza.kdl.
const RECIPES: Cards = &[
    ("look", &[
        ("hide the bar", "bar { autohide on }  The bar becomes a thin strip at the top edge and grows when the pointer touches it."),
        ("change the wallpaper by itself", "look { slideshow 30 }  Every 30 minutes the next picture in the wallpaper's folder recolors the sky. 0 is off."),
        ("see the weather", "weather { place \"Lisbon\" }, then turn on the weather card in settings, widgets. Without a place it guesses from your address."),
        ("see what is coming up", "calendar { source \"~/cal.ics\" }, then turn on the agenda card. A calendar's private address works too."),
    ]),
    ("windows", &[
        ("tile every star", "layout { tile on; max 3; gap 10 }  The first 3 windows of a star are laid out for you. Super+t flips one star."),
        ("send an app to a star", "rule app=\"steam\" star=\"games\" mode=\"maximize\"  The first rule that matches wins. seiza debug-windows shows app ids."),
        ("size a window", "rule app=\"pavucontrol\" w=700 h=500  Give both w and h. title=\"part of the title\" matches by title instead."),
    ]),
    ("session", &[
        ("lock when you leave", "idle { dim 120; lock 300; off 600 }  Seconds without input. 0 is never. Anything watching video keeps it awake."),
        ("choose what starts", "autostart { spawn \"seiza-bar\"; spawn \"seiza-starfield\" }  Leave it out to get the usual ones. An empty block starts nothing."),
        ("check your file", "seiza-conf check  names mistakes by line. Saving the file reloads it by itself."),
    ]),
    ("keys", &[
        ("bind a key", "binds { Super+Return { spawn \"kitty\" } }  Several actions: Super+g { goto 1; spawn \"kitty\"; }"),
        ("name a few actions", "def my-thing { goto 1; spawn \"notify-send hi\"; }  then use my-thing in a bind."),
    ]),
];

const FILES: Cards = &[
    ("in ~/.config/seiza", &[
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
    ]),
    ("yours to make", &[
        ("figures/NAME.fig", "your own constellation templates."),
        ("*.const", "saved constellations."),
    ]),
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

/// How the keys page is laid out for the current width.
struct KeyPlan {
    cols: usize,
    col_w: f32,
    col: Vec<usize>,      // the column of each group
    used: Vec<f32>,       // the height of each column
    heights: Vec<f32>,    // the height of each group's card
    wrap: Vec<Vec<bool>>, // per group, per row: keycaps under the words
}

/// What a key is called on the page.
fn shown(k: &str) -> &str {
    match k {
        "<Direction>" => "\u{2190} \u{2191} \u{2193} \u{2192}",
        "<Number>" => "number",
        _ => k,
    }
}

/// The keys page's items as (group, [(keys, what)]).
fn key_groups(keys: &[Item]) -> Vec<(&str, Vec<(&[String], &str)>)> {
    let mut groups: Vec<(&str, Vec<(&[String], &str)>)> = vec![];
    for it in keys {
        match it {
            Item::Head(g) => groups.push((g, vec![])),
            Item::Bind(caps, what) => groups.last_mut().expect("a binding follows its heading").1.push((caps, what)),
        }
    }
    groups
}

pub struct Ui {
    pub page: usize,
    pub scroll: usize, // steps scrolled off the top
    query: String,     // what is typed in the search pill
    hover: Option<usize>, // the page star under the pointer
    glide: kit::Glide, // the big star moving to another page
    stale: bool,                   // what is in `content` is out of date
    content: Option<Canvas>,       // the last frame drawn at full visibility
    back: Option<Canvas>,          // the card's plate, which only changes with the screen size
    body: Option<Canvas>,          // the page's body, which only changes with the page, scroll, keys and size
    body_stale: bool,
    w: f32, // the card's size: most of the screen
    h: f32,
    keys: Vec<Item>,
    t22: Text,
    t15: Text,
    t13: Text,
}

/// The options table as cards: one per section, its blurb first, then each option with its range and default.
fn options_cards() -> Vec<(String, Vec<(String, String)>)> {
    use seiza_common::conf::{Kind, OPTS, SECTIONS};
    SECTIONS
        .iter()
        .map(|(sec, what)| {
            let mut rows = vec![(String::new(), what.to_string())];
            for o in OPTS.iter().filter(|o| o.section == *sec) {
                let hint = match &o.kind {
                    Kind::Flag => "on or off".to_string(),
                    Kind::Num(lo, hi) => format!("{lo} to {hi}"),
                    Kind::Pick(list) => list.join(", "),
                    Kind::Text => "text".to_string(),
                };
                let default = if o.default.is_empty() { "empty".to_string() } else { o.default.to_string() };
                rows.push((o.key.to_string(), format!("{}  ({hint}; default {default})", o.doc)));
            }
            (sec.to_string(), rows)
        })
        .collect()
}

/// The cards of a text page (every page but keys).
fn text_cards(page: usize) -> Vec<(String, Vec<(String, String)>)> {
    let own = |g: Cards| g.iter().map(|(t, rows)| (t.to_string(), rows.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect())).collect();
    match page {
        1 => own(COMMANDS),
        2 => own(GUIDE),
        3 => own(RECIPES),
        4 => options_cards(),
        _ => own(FILES),
    }
}

/// Deal cards of these heights into `cols` columns, each into the shortest column: the tallest first when
/// `tallest_first` (they come out level), else in their own order (they read left to right).
/// Returns each card's column and each column's height.
fn deal(cols: usize, heights: &[f32], tallest_first: bool) -> (Vec<usize>, Vec<f32>) {
    let mut by_height: Vec<usize> = (0..heights.len()).collect();
    if tallest_first {
        by_height.sort_by(|&a, &b| heights[b].total_cmp(&heights[a]));
    }
    let (mut col, mut used) = (vec![0usize; heights.len()], vec![0.0f32; cols]);
    for i in by_height {
        let c = (0..cols).min_by(|&a, &b| used[a].total_cmp(&used[b])).unwrap_or(0);
        col[i] = c;
        used[c] += heights[i] + GAP;
    }
    (col, used)
}

/// How a text page is laid out for the current width.
struct TextPlan {
    cols: usize,
    col_w: f32,
    col: Vec<usize>,
    used: Vec<f32>,
    heights: Vec<f32>,
    cards: Vec<(String, Vec<(String, String)>)>,
    lines: Vec<Vec<Vec<String>>>, // per card, per row: the description wrapped to the card
}

pub fn keys_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("seiza-keys")
}

impl Ui {
    pub fn new() -> Ui {
        let font = load_font();
        let mut ui = Ui {
            page: 0,
            scroll: 0,
            query: String::new(),
            hover: None,
            glide: kit::Glide::default(),
            stale: true,
            content: None,
            back: None,
            body: None,
            body_stale: true,
            w: W0,
            h: H0,
            keys: vec![],
            t22: Text::new(font.clone(), 24.0),
            t15: Text::new(font.clone(), 15.0),
            t13: Text::new(font, 13.0),
        };
        ui.reload_keys();
        ui
    }

    pub fn reload_keys(&mut self) {
        self.keys = key_items(&std::fs::read_to_string(keys_path()).unwrap_or_default());
        self.stale = true;
        self.body_stale = true;
    }

    fn cw(&self) -> f32 {
        self.w - 2.0 * PAD
    }

    fn body_h(&self) -> f32 {
        self.h - TOP - FOOT
    }

    pub fn searching(&self) -> bool {
        !self.query.is_empty()
    }

    fn query_changed(&mut self) {
        self.scroll = 0;
        self.stale = true;
        self.body_stale = true;
    }

    /// Add typed text to the search.
    pub fn type_str(&mut self, s: &str) {
        self.query.push_str(s);
        self.query_changed();
    }

    pub fn backspace(&mut self) {
        if self.query.pop().is_some() {
            self.query_changed();
        }
    }

    /// Empty the search; false if there was nothing in it.
    pub fn clear_search(&mut self) -> bool {
        let had = self.searching();
        self.query.clear();
        if had {
            self.query_changed();
        }
        had
    }

    /// The rows of every page that match all the words typed, one card per page. Keys rows are the
    /// binding's meaning with its keys as the description.
    fn search_cards(&self) -> Vec<(String, Vec<(String, String)>)> {
        let words: Vec<String> = self.query.to_lowercase().split_whitespace().map(String::from).collect();
        let keys = key_groups(&self.keys)
            .into_iter()
            .flat_map(|(_, binds)| binds)
            .map(|(caps, what)| (what.to_string(), caps.iter().map(|c| shown(c)).collect::<Vec<_>>().join(" + ")));
        let mut out = vec![];
        for page in 0..PAGES.len() {
            let rows: Vec<(String, String)> = if page == 0 { keys.clone().collect() } else { text_cards(page).into_iter().flat_map(|(_, r)| r).collect() };
            let hits: Vec<(String, String)> = rows
                .into_iter()
                .filter(|(n, d)| {
                    if n.is_empty() && page != 0 {
                        return false; // a section's blurb, not a thing to find
                    }
                    let hay = format!("{n} {d}").to_lowercase();
                    words.iter().all(|w| hay.contains(w.as_str()))
                })
                .collect();
            if !hits.is_empty() {
                out.push((PAGES[page].to_string(), hits));
            }
        }
        out
    }

    /// The cards of the text layout: the search results, else the page's own.
    fn shown_cards(&self) -> Vec<(String, Vec<(String, String)>)> {
        if self.searching() { self.search_cards() } else { text_cards(self.page) }
    }

    /// Whether the body is the keycap cards of the keys page (not a text layout).
    fn keys_body(&self) -> bool {
        self.page == 0 && !self.searching()
    }

    /// How many columns of cards fit (MINCOL each) and how wide they are.
    fn columns(&self) -> (usize, f32) {
        let cw = self.cw();
        let cols = (((cw + GAP) / (MINCOL + GAP)) as usize).clamp(1, 3);
        (cols, (cw - GAP * (cols - 1) as f32) / cols as f32)
    }

    /// `text` broken into lines no wider than `room`.
    fn wrap(&mut self, text: &str, room: f32) -> Vec<String> {
        let mut lines: Vec<String> = vec![];
        for word in text.split(' ') {
            match lines.last_mut() {
                Some(l) if self.t13.width(&format!("{l} {word}")) <= room => {
                    l.push(' ');
                    l.push_str(word);
                }
                _ => lines.push(word.to_string()),
            }
        }
        lines
    }

    /// (steps in the page, steps that fit); a step is one keys row, and the page is as tall as its tallest column.
    fn extent(&mut self) -> (usize, usize) {
        let used = if self.keys_body() { self.key_plan().used } else { self.text_plan().used };
        let tallest = used.iter().cloned().fold(0.0, f32::max);
        ((tallest / ROW1).ceil() as usize, (self.body_h() / ROW1) as usize)
    }

    pub fn scroll_by(&mut self, d: isize) {
        let (total, visible) = self.extent();
        let d = d * 2; // a step is one short row
        self.scroll = (self.scroll as isize + d).clamp(0, total.saturating_sub(visible) as isize) as usize;
        self.stale = true;
        self.body_stale = true;
    }

    pub fn go(&mut self, page: usize) {
        let page = page.min(PAGES.len() - 1);
        if page != self.page {
            self.glide.start(self.nav_pos());
        }
        self.page = page;
        self.scroll = 0;
        self.stale = true;
        self.body_stale = true;
    }

    /// Where the big star is, as a (fractional) page number.
    fn nav_pos(&self) -> f32 {
        self.glide.pos(self.page)
    }

    /// True while a frame is still owed: the star is moving, or what is shown is out of date.
    pub fn pending(&self) -> bool {
        self.stale
    }

    /// Size the card to the screen: most of it, so the desktop still shows around it.
    fn fit(&mut self, w: f32, h: f32) {
        self.w = (w * 0.83).clamp(1000.0, 1700.0).min(w - 40.0);
        self.h = (h * 0.84).clamp(620.0, 940.0).min(h - 40.0);
    }

    /// The card sits a little below the middle, under the bar.
    fn origin(&self, w: f32, h: f32) -> (f32, f32) {
        (((w - self.w) / 2.0).round(), ((h - self.h) / 2.0 + 18.0).round())
    }

    /// The row of page stars across the card's top.
    fn thread(&self, ox: f32, oy: f32) -> Thread {
        Thread::new(ox + 170.0, ox + self.w - PAD - PILL_W - 30.0, oy + 40.0, PAGES.len())
    }

    /// A click: on a page star it switches page. Returns true if it changed something.
    pub fn press(&mut self, x: f32, y: f32, w: f32, h: f32) -> bool {
        match self.nav_at(x, y, w, h) {
            Some(i) => {
                self.go(i);
                true
            }
            None => false,
        }
    }

    /// The page star under (x, y), if any.
    fn nav_at(&mut self, x: f32, y: f32, w: f32, h: f32) -> Option<usize> {
        self.fit(w, h);
        let (ox, oy) = self.origin(w, h);
        self.thread(ox, oy).hit(x, y)
    }

    /// The pointer moved to `at` (None: it left); true if the highlight changed and a redraw is due.
    pub fn hover(&mut self, at: Option<(f32, f32)>, w: f32, h: f32) -> bool {
        let now = at.and_then(|(x, y)| self.nav_at(x, y, w, h));
        let changed = std::mem::replace(&mut self.hover, now) != now;
        self.stale |= changed;
        changed
    }

    /// How wide a row of keycaps is.
    fn caps_w(&mut self, caps: &[String]) -> f32 {
        caps.iter().map(|c| kit::keycap_w(&mut self.t13, shown(c))).sum::<f32>() + CAP_SEP * caps.len().saturating_sub(1) as f32
    }

    /// The keys page as cards in as many columns as are wide enough (MINCOL); each group whole, the tallest
    /// first, always into the shortest column; within a column the groups keep their order.
    fn key_plan(&mut self) -> KeyPlan {
        let keys = std::mem::take(&mut self.keys); // so the text can be measured while these are borrowed
        let groups = key_groups(&keys);
        let (cols, col_w) = self.columns();
        let (mut wrap, mut heights) = (vec![], vec![]);
        for (_, binds) in &groups {
            let mut h = kit::CARD_HEAD + 6.0;
            let mut flags = vec![];
            for (caps, what) in binds {
                let under = self.t15.width(what) + 24.0 + self.caps_w(caps) > col_w - 36.0;
                h += if under { ROW2 } else { ROW1 };
                flags.push(under);
            }
            wrap.push(flags);
            heights.push(h);
        }
        let (col, used) = deal(cols, &heights, true);
        drop(groups);
        self.keys = keys;
        KeyPlan { cols, col_w, col, used, heights, wrap }
    }

    /// The keys page's cards, scrolled by `self.scroll` steps, drawn at the body's own origin.
    fn draw_keys(&mut self, cv: &mut Canvas) {
        let p = palette();
        let (plan, bh) = (self.key_plan(), self.body_h());
        let keys = std::mem::take(&mut self.keys);
        let groups = key_groups(&keys);
        let mut y = vec![-(self.scroll as f32) * ROW1; plan.cols];
        for (i, (head, binds)) in groups.iter().enumerate() {
            let (c, h) = (plan.col[i], plan.heights[i]);
            let (x, top) = (c as f32 * (plan.col_w + GAP), y[c]);
            y[c] += h + GAP;
            if top + h < 0.0 || top > bh {
                continue; // not on screen
            }
            kit::card(cv, &mut self.t13, x, top, plan.col_w, h, &head.to_lowercase());
            let mut ry = top + kit::CARD_HEAD;
            for (k, (caps, what)) in binds.iter().enumerate() {
                let under = plan.wrap[i][k];
                if k > 0 {
                    cv.round_rect(x + 1.0, ry, plan.col_w - 2.0, 1.0, 0.0, rgba(p.text, 0.06));
                }
                let (word_y, cap_x, cap_y) = if under { (ry + 28.0, x + 18.0, ry + 36.0) } else { (ry + 27.0, x + plan.col_w - 18.0 - self.caps_w(caps), ry + 10.0) };
                self.t15.draw(cv, x + 18.0, word_y, what, rgba(p.text, 0.92));
                let mut kx = cap_x;
                for (j, cap) in caps.iter().enumerate() {
                    if j > 0 {
                        self.t13.draw(cv, kx + 5.0, cap_y + 17.0, "+", rgba(p.text, 0.45));
                        kx += CAP_SEP;
                    }
                    kx = kit::keycap(cv, &mut self.t13, kx, cap_y, shown(cap));
                }
                ry += if under { ROW2 } else { ROW1 };
            }
        }
        drop(groups);
        self.keys = keys;
    }

    /// The text pages as cards: each row a name (when it has one) with its description wrapped under it.
    fn text_plan(&mut self) -> TextPlan {
        let cards = self.shown_cards();
        let (cols, col_w) = self.columns();
        let (mut lines, mut heights) = (vec![], vec![]);
        for (_, rows) in &cards {
            let mut h = kit::CARD_HEAD + 4.0;
            let mut card_lines = vec![];
            for (name, desc) in rows {
                let l = self.wrap(desc, col_w - 36.0);
                h += 12.0 + if name.is_empty() { 0.0 } else { 24.0 } + l.len() as f32 * 18.0 + 12.0;
                card_lines.push(l);
            }
            lines.push(card_lines);
            heights.push(h);
        }
        let (col, used) = deal(cols, &heights, false);
        TextPlan { cols, col_w, col, used, heights, cards, lines }
    }

    /// The text pages' cards, scrolled by `self.scroll` steps, drawn at the body's own origin.
    fn draw_text(&mut self, cv: &mut Canvas) {
        let p = palette();
        let (plan, bh) = (self.text_plan(), self.body_h());
        let mut y = vec![-(self.scroll as f32) * ROW1; plan.cols];
        for (i, (title, rows)) in plan.cards.iter().enumerate() {
            let (c, h) = (plan.col[i], plan.heights[i]);
            let (x, top) = (c as f32 * (plan.col_w + GAP), y[c]);
            y[c] += h + GAP;
            if top + h < 0.0 || top > bh {
                continue; // not on screen
            }
            kit::card(cv, &mut self.t13, x, top, plan.col_w, h, title);
            let mut ry = top + kit::CARD_HEAD;
            for (k, (name, _)) in rows.iter().enumerate() {
                if k > 0 {
                    cv.round_rect(x + 1.0, ry, plan.col_w - 2.0, 1.0, 0.0, rgba(p.text, 0.06));
                }
                let mut by = ry + 12.0;
                if !name.is_empty() {
                    self.t15.draw(cv, x + 18.0, by + 14.0, name, rgba(p.star, 0.9));
                    by += 24.0;
                }
                for line in &plan.lines[i][k] {
                    self.t13.draw(cv, x + 18.0, by + 12.0, line, rgba(p.text, if name.is_empty() { 0.5 } else { 0.7 }));
                    by += 18.0;
                }
                ry = by + 12.0;
            }
        }
    }

    /// The frame at visibility `e` (0 = gone, 1 = fully there): the card rises from below while it fades in.
    pub fn render(&mut self, w: usize, h: usize, e: f32) -> Canvas {
        self.fit(w as f32, h as f32);
        let g = self.glide.active();
        if self.stale || g || self.content.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            self.content = Some(self.draw_card(w, h));
            self.stale = g; // a glide that was still going when this was drawn owes one more frame
            if !g {
                self.glide.clear();
            }
        }
        let oy = self.origin(w as f32, h as f32).1;
        kit::appear(self.content.as_ref().expect("drawn above"), e, (oy as usize).saturating_sub(24)..(oy + self.h) as usize + 24)
    }

    /// The card's plate: a gold glow and edge round the window, its footer line and the words that never change.
    fn plate(&mut self, w: usize, h: usize) -> Canvas {
        let p = palette();
        let (ox, oy) = self.origin(w as f32, h as f32);
        let mut b = kit::window_plate(w, h, (ox, oy), (self.w, self.h), FOOT);
        self.t22.draw(&mut b, ox + PAD, oy + 46.0, "help", rgba(p.text, 0.95));
        let base = oy + self.h - 22.0;
        self.t13.draw(&mut b, ox + PAD, base, "click a star to change page  \u{b7}  type to search all of it  \u{b7}  Up/Down or wheel scrolls", rgba(p.text, 0.4));
        let quit = "Esc: clear / close";
        let qw = self.t13.width(quit);
        self.t13.draw(&mut b, ox + self.w - PAD - qw, base, quit, rgba(p.text, 0.4));
        b
    }

    fn draw_card(&mut self, w: usize, h: usize) -> Canvas {
        let p = palette();
        let (ox, oy) = self.origin(w as f32, h as f32);
        if self.back.as_ref().is_none_or(|b| b.w != w || b.h != h) {
            self.back = Some(self.plate(w, h));
        }
        let mut cv = self.back.clone().expect("made above");
        let (pos, hover) = (if self.searching() { -9.0 } else { self.nav_pos() }, self.hover); // no big star: the results are not one page
        self.thread(ox, oy).draw(&mut cv, &mut self.t13, &PAGES, pos, hover);

        let (title, blurb) = if self.searching() {
            let n: usize = self.search_cards().iter().map(|(_, r)| r.len()).sum();
            let found = if n == 0 { "nothing matches that".to_string() } else { format!("{n} match{} across every page", if n == 1 { "" } else { "es" }) };
            ("search".to_string(), found)
        } else {
            let blurb = match self.page {
                0 => format!("live from {}: edit it and this updates.", config::path("keys").display()),
                1 => "type these in a terminal, or bind them to a key.".into(),
                2 => "the words Seiza uses.".into(),
                3 => format!("copy a line into {}.", config::path("seiza.kdl").display()),
                4 => "every setting seiza.kdl takes; seiza-conf docs prints the same.".into(),
                _ => format!("in {}", config::path("").display()),
            };
            (PAGES[self.page].to_string(), blurb)
        };
        self.t22.draw(&mut cv, ox + PAD, oy + 122.0, &title, rgba(p.text, 1.0));
        self.t13.draw(&mut cv, ox + PAD, oy + 146.0, &blurb, rgba(p.text, 0.55));
        kit::search_pill(&mut cv, &mut self.t15, ox + self.w - PAD - PILL_W, oy + 23.0, &self.query, "ask the sky..");

        let (cw, bh) = (self.cw() as usize, self.body_h() as usize);
        if self.body_stale || self.body.as_ref().is_none_or(|b| b.w != cw || b.h != bh) {
            let mut body = Canvas { w: cw, h: bh, px: vec![0; cw * bh] };
            if self.keys_body() {
                self.draw_keys(&mut body);
            } else {
                self.draw_text(&mut body);
            }
            self.body = Some(body);
            self.body_stale = false;
        }
        cv.over(self.body.as_ref().expect("made above"), (ox + PAD) as i32, (oy + TOP) as i32);

        let (total, visible) = self.extent();
        if total > visible {
            let (track, frac) = (bh as f32, visible as f32 / total as f32);
            let top = self.scroll as f32 / total as f32;
            cv.round_rect(ox + self.w - 14.0, oy + TOP, 4.0, track, 2.0, rgba(p.text, 0.1));
            cv.round_rect(ox + self.w - 14.0, oy + TOP + track * top, 4.0, track * frac, 2.0, rgba(p.star, 0.9));
        }
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
        let cards = options_cards();
        let rows: Vec<&(String, String)> = cards.iter().flat_map(|(_, r)| r).collect();
        for key in ["autohide", "slideshow", "place", "source", "tile"] {
            assert!(rows.iter().any(|(l, _)| l == key), "{key} missing");
        }
        assert!(rows.iter().any(|(l, r)| l == "radius" && r.contains("default 14")));
        let mut ui = Ui::new();
        ui.go(4);
        let plan = ui.text_plan();
        assert!(plan.heights.iter().all(|h| *h > kit::CARD_HEAD));
        assert_eq!(plan.lines.len(), cards.len());
    }

    #[test]
    fn every_text_page_has_cards_whose_rows_all_get_a_line() {
        let mut ui = Ui::new();
        for page in 1..PAGES.len() {
            ui.go(page);
            let plan = ui.text_plan();
            assert!(!plan.cards.is_empty(), "page {page}");
            assert!(plan.lines.iter().flatten().all(|l| !l.is_empty()), "page {page}: a row with no text");
        }
    }

    #[test]
    fn the_keys_page_deals_its_cards_into_as_many_columns_as_fit() {
        let bind = |n: usize| (0..n).map(|i| format!("Super+{}Alt = seiza pan {i} 0\n", i)).collect::<String>();
        let mut ui = Ui::new();
        ui.keys = key_items(&(bind(10) + "Super+h = seiza-help\nSuper+q = seiza close-window\nSuper+m = seiza maximize\nSuper+l = seiza-launcher\n"));
        ui.fit(1920.0, 1080.0);
        assert_eq!(ui.key_plan().cols, 3);
        ui.fit(1024.0, 600.0);
        let plan = ui.key_plan();
        assert_eq!(plan.cols, 2);
        assert!(plan.col_w >= MINCOL);
        ui.go(0);
        let (total, visible) = ui.extent();
        ui.scroll_by(1000);
        assert_eq!(ui.scroll, total.saturating_sub(visible));
    }

    #[test]
    fn keycaps_that_do_not_fit_beside_the_words_go_under_them() {
        let mut ui = Ui::new();
        ui.keys = key_items("Super+Ctrl+Left = seiza resize-window -50 0\nSuper+Ctrl+Right = seiza resize-window 50 0\nSuper+q = seiza close-window\n");
        ui.w = 1000.0; // wide: one column of 940
        assert_eq!(ui.key_plan().cols, 2);
        ui.w = 600.0; // narrow: one column
        let plan = ui.key_plan();
        assert_eq!(plan.cols, 1);
        assert!(plan.wrap[0].iter().all(|&u| !u), "room enough: beside");
        ui.w = 300.0;
        assert!(ui.key_plan().wrap[0].iter().any(|&u| u), "no room: under");
    }

    #[test]
    fn the_pointer_highlights_a_page_star_once() {
        let mut ui = Ui::new();
        ui.fit(1920.0, 1080.0);
        let (ox, oy) = ui.origin(1920.0, 1080.0);
        let (sx, sy) = ui.thread(ox, oy).star(1);
        assert!(ui.hover(Some((sx, sy + 10.0)), 1920.0, 1080.0));
        assert_eq!(ui.hover, Some(1));
        assert!(!ui.hover(Some((sx + 1.0, sy + 12.0)), 1920.0, 1080.0), "same star, no redraw");
        assert!(ui.hover(None, 1920.0, 1080.0));
    }

    #[test]
    fn the_big_star_glides_then_settles() {
        let mut ui = Ui::new();
        ui.render(1280, 720, 1.0);
        assert!(!ui.pending(), "nothing moving, nothing owed");
        ui.go(2);
        assert!(ui.pending() && ui.nav_pos() < 1.0, "just left entry 0");
        ui.render(1280, 720, 1.0);
        assert!(ui.pending(), "still gliding: another frame is owed");
        std::thread::sleep(std::time::Duration::from_millis((kit::GLIDE_SECS * 1000.0) as u64 + 40));
        ui.render(1280, 720, 1.0);
        assert!(!ui.pending() && ui.nav_pos() == 2.0, "the last frame lands on the entry");
    }

    #[test]
    fn typing_searches_every_page() {
        let mut ui = Ui::new();
        ui.keys = key_items("Super+q = seiza close-window\nSuper+h = seiza-help\n");
        ui.type_str("close");
        let cards = ui.shown_cards();
        let titles: Vec<&str> = cards.iter().map(|(t, _)| t.as_str()).collect();
        assert!(titles.contains(&"keys") && titles.contains(&"commands"), "{titles:?}");
        let keys = &cards.iter().find(|(t, _)| t == "keys").unwrap().1;
        assert_eq!(keys, &[("close the window".to_string(), "Super + q".to_string())]);
        ui.type_str(" window");
        assert!(ui.search_cards().iter().all(|(_, r)| r.iter().all(|(n, d)| format!("{n} {d}").to_lowercase().contains("window"))), "every word must match");
        ui.type_str(" zzzz");
        assert!(ui.search_cards().is_empty());
        assert!(ui.text_plan().cards.is_empty(), "nothing found lays out nothing");
    }

    #[test]
    fn escape_clears_the_search_before_it_closes() {
        let mut ui = Ui::new();
        assert!(!ui.clear_search(), "nothing to clear: the caller closes");
        ui.type_str("ab");
        ui.backspace();
        assert_eq!(ui.query, "a");
        assert!(ui.clear_search() && !ui.searching());
        assert!(ui.keys_body(), "back to the page");
    }

    #[test]
    fn scroll_stays_in_range() {
        let mut ui = Ui::new();
        ui.fit(1280.0, 720.0);
        ui.go(1);
        let (total, visible) = ui.extent();
        assert!(total > visible, "the commands page does not fit 1280x720");
        ui.scroll_by(1000);
        assert_eq!(ui.scroll, total - visible);
        ui.scroll_by(-1000);
        assert_eq!(ui.scroll, 0);
    }
}
