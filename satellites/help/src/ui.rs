//! The help pages: content, layout and drawing, with no Wayland in it so `--dump` and the
//! tests can run it. The key bindings come from the file the compositor rewrites on every
//! (re)load of ~/.config/seiza/keys, so they are never stale; the other pages are written here.

use seiza_common::{
    config,
    draw::{shared_font, Canvas, Text},
    kit::{self, Thread, PILL_W},
    palette::{palette, rgba},
};

use crate::pages::{self, Fonts, Page};

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
    extra: Vec<Box<dyn Page>>, // the pages after the keybinds
    page_total: Option<f32>,   // how tall the current extra page said it is
    t22: Text,
    t15: Text,
    t13: Text,
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
        let font = shared_font();
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
            extra: pages::extra_pages(),
            page_total: None,
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

    pub fn page_count(&self) -> usize {
        1 + self.extra.len()
    }

    /// The label under each star: the keybinds, then the other pages.
    fn names(&self) -> Vec<&'static str> {
        std::iter::once("keybinds").chain(self.extra.iter().map(|p| p.name())).collect()
    }

    /// The index in `extra` of the page being shown, if it is one of those (and not the search results).
    fn extra_index(&self) -> Option<usize> {
        (self.page > 0 && !self.searching()).then(|| self.page - 1)
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
        for page in 0..self.page_count() {
            let (name, rows): (&str, Vec<(String, String)>) = if page == 0 { ("keybinds", keys.clone().collect()) } else { (self.extra[page - 1].name(), self.extra[page - 1].rows()) };
            let hits: Vec<(String, String)> = rows
                .into_iter()
                .filter(|(n, d)| {
                    let hay = format!("{n} {d}").to_lowercase();
                    words.iter().all(|w| hay.contains(w.as_str()))
                })
                .collect();
            if !hits.is_empty() {
                out.push((name.to_string(), hits));
            }
        }
        out
    }

    /// The cards of the text layout, which only the search results use.
    fn shown_cards(&self) -> Vec<(String, Vec<(String, String)>)> {
        if self.searching() { self.search_cards() } else { vec![] }
    }

    /// Whether the body is the keycap cards of the keybinds page (not a text layout or another page).
    fn keys_body(&self) -> bool {
        self.page == 0 && !self.searching()
    }

    /// How many columns of cards fit (MINCOL each) and how wide they are.
    fn columns(&self) -> (usize, f32) {
        let cw = self.cw();
        let cols = (((cw + GAP) / (MINCOL + GAP)) as usize).clamp(1, 3);
        (cols, (cw - GAP * (cols - 1) as f32) / cols as f32)
    }

    /// (steps in the page, steps that fit); a step is one keys row, and the page is as tall as its tallest column.
    fn extent(&mut self) -> (usize, usize) {
        let tallest = if self.keys_body() {
            self.key_plan().used.iter().cloned().fold(0.0, f32::max)
        } else if let Some(i) = self.extra_index() {
            self.extra_total(i)
        } else {
            self.text_plan().used.iter().cloned().fold(0.0, f32::max)
        };
        let total = (tallest / ROW1).ceil() as usize;
        // a page that fits needs no scrolling: compare in pixels, or one that is exactly as tall as the body would
        // count as a step too long and show a scrollbar
        let visible = if tallest <= self.body_h() { total } else { (self.body_h() / ROW1) as usize };
        (total, visible)
    }

    /// How tall page `i` of `extra` says it is: what its last draw returned, or one drawn on the side.
    fn extra_total(&mut self, i: usize) -> f32 {
        if let Some(t) = self.page_total {
            return t;
        }
        let (cw, bh) = (self.cw() as usize, self.body_h() as usize);
        let mut scratch = Canvas { w: cw, h: bh, px: vec![0; cw * bh] };
        let t = self.extra[i].draw(&mut scratch, &mut Fonts { title: &mut self.t22, body: &mut self.t15, small: &mut self.t13 }, 0.0);
        self.page_total = Some(t);
        t
    }

    pub fn scroll_by(&mut self, d: isize) {
        let (total, visible) = self.extent();
        let d = d * 2; // a step is one short row
        self.scroll = (self.scroll as isize + d).clamp(0, total.saturating_sub(visible) as isize) as usize;
        self.stale = true;
        self.body_stale = true;
    }

    pub fn go(&mut self, page: usize) {
        let page = page.min(self.page_count() - 1);
        if page != self.page {
            self.glide.start(self.nav_pos());
        }
        self.page = page;
        self.scroll = 0;
        self.page_total = None;
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
        Thread::new(ox + 170.0, ox + self.w - PAD - PILL_W - 30.0, oy + 40.0, self.page_count())
    }

    /// A click: on a page star it switches page. Returns true if it changed something.
    pub fn press(&mut self, x: f32, y: f32, w: f32, h: f32) -> bool {
        match self.nav_at(x, y, w, h) {
            Some(i) => {
                self.go(i);
                true
            }
            None => self.pointer(x, y, true, w, h),
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
        let page = at.is_some_and(|(x, y)| self.pointer(x, y, false, w, h)); // a page may react to the pointer too
        changed || page
    }

    /// The pointer at a screen point, passed to the page being shown if the point is over its body: in content
    /// coordinates (the scroll added). True if the page needs drawing again.
    pub fn pointer(&mut self, x: f32, y: f32, press: bool, w: f32, h: f32) -> bool {
        let Some(i) = self.extra_index() else { return false };
        self.fit(w, h);
        let (ox, oy) = self.origin(w, h);
        let (bx, by) = (x - ox - PAD, y - oy - TOP);
        if bx < 0.0 || bx >= self.cw() || by < 0.0 || by >= self.body_h() {
            return false;
        }
        let changed = self.extra[i].pointer(bx, by + self.scroll as f32 * ROW1, press);
        if changed {
            self.stale = true;
            self.body_stale = true;
        }
        changed
    }

    /// About twice a second: the page being shown says whether it changed by itself. True if a frame is due.
    pub fn tick(&mut self) -> bool {
        let Some(i) = self.extra_index() else { return false };
        let changed = self.extra[i].tick();
        if changed {
            self.stale = true;
            self.body_stale = true;
        }
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
                let l = kit::wrap(&mut self.t13, desc, col_w - 36.0);
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
        let names = self.names();
        self.thread(ox, oy).draw(&mut cv, &mut self.t13, &names, pos, hover);

        let (title, blurb) = if self.searching() {
            let n: usize = self.search_cards().iter().map(|(_, r)| r.len()).sum();
            let found = if n == 0 { "nothing matches that".to_string() } else { format!("{n} match{} across every page", if n == 1 { "" } else { "es" }) };
            ("search".to_string(), found)
        } else {
            match self.extra_index() {
                Some(i) => (self.extra[i].name().to_string(), self.extra[i].blurb()),
                None => ("keybinds".to_string(), format!("live from {}: edit it and this updates.", config::path("keys").display())),
            }
        };
        self.t22.draw(&mut cv, ox + PAD, oy + 122.0, &title, rgba(p.text, 1.0));
        self.t13.draw(&mut cv, ox + PAD, oy + 146.0, &blurb, rgba(p.text, 0.55));
        kit::search_pill(&mut cv, &mut self.t15, ox + self.w - PAD - PILL_W, oy + 23.0, &self.query, "ask the sky..");

        let (cw, bh) = (self.cw() as usize, self.body_h() as usize);
        if self.body_stale || self.body.as_ref().is_none_or(|b| b.w != cw || b.h != bh) {
            let mut body = Canvas { w: cw, h: bh, px: vec![0; cw * bh] };
            if self.keys_body() {
                self.draw_keys(&mut body);
            } else if let Some(i) = self.extra_index() {
                let total = self.extra[i].draw(&mut body, &mut Fonts { title: &mut self.t22, body: &mut self.t15, small: &mut self.t13 }, self.scroll as f32 * ROW1);
                self.page_total = Some(total);
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

    /// A page the tests control: tall, with a searchable row, and it records what the pointer did and how often it was ticked.
    struct Probe {
        seen: std::rc::Rc<std::cell::RefCell<Vec<(f32, f32, bool)>>>,
        ticks: std::rc::Rc<std::cell::Cell<u32>>,
    }

    impl Page for Probe {
        fn name(&self) -> &'static str {
            "probe"
        }
        fn blurb(&self) -> String {
            "a page for the tests".into()
        }
        fn draw(&mut self, _: &mut Canvas, _: &mut Fonts, _: f32) -> f32 {
            3000.0
        }
        fn rows(&self) -> Vec<(String, String)> {
            vec![("a probing thing".into(), "found by the search".into())]
        }
        fn tick(&mut self) -> bool {
            self.ticks.set(self.ticks.get() + 1);
            true
        }
        fn pointer(&mut self, x: f32, y: f32, press: bool) -> bool {
            self.seen.borrow_mut().push((x, y, press));
            press
        }
    }

    /// A Ui with a Probe as its last page, fitted to 1920x1080; returns it, the probe's page number and its records.
    fn ui_with_probe() -> (Ui, usize, std::rc::Rc<std::cell::RefCell<Vec<(f32, f32, bool)>>>, std::rc::Rc<std::cell::Cell<u32>>) {
        let (seen, ticks) = (Default::default(), Default::default());
        let mut ui = Ui::new();
        ui.extra.push(Box::new(Probe { seen: std::rc::Rc::clone(&seen), ticks: std::rc::Rc::clone(&ticks) }));
        ui.fit(1920.0, 1080.0);
        let page = ui.page_count() - 1;
        (ui, page, seen, ticks)
    }

    #[test]
    fn help_has_the_keybinds_and_the_pages_after_them() {
        let ui = Ui::new();
        assert_eq!(ui.names()[0], "keybinds");
        assert_eq!(&ui.names()[1..], &["elements", "voyager", "yoru & seiza"]);
        assert_eq!(ui.page_count(), 4);
    }

    #[test]
    fn typing_searches_every_page() {
        let (mut ui, _, _, _) = ui_with_probe();
        ui.keys = key_items("Super+q = seiza close-window\nSuper+h = seiza-help\n");
        ui.type_str("close the window"); // words only this binding has, whatever the other pages say
        let cards = ui.shown_cards();
        assert_eq!(cards.len(), 1, "{cards:?}");
        assert_eq!(cards[0].0, "keybinds");
        assert_eq!(cards[0].1, [("close the window".to_string(), "Super + q".to_string())]);
        ui.clear_search();
        ui.type_str("probing");
        let cards = ui.shown_cards();
        assert_eq!(cards.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["probe"], "a page's rows are found too");
        ui.type_str(" zzzz");
        assert!(ui.search_cards().is_empty());
        assert!(ui.text_plan().cards.is_empty(), "nothing found lays out nothing");
    }

    #[test]
    fn a_page_gets_the_pointer_in_content_coordinates_and_only_over_its_body() {
        let (mut ui, page, seen, _) = ui_with_probe();
        ui.go(page);
        let (ox, oy) = ui.origin(1920.0, 1080.0);
        let (bx, by) = (ox + PAD, oy + TOP);
        assert!(ui.press(bx + 100.0, by + 50.0, 1920.0, 1080.0), "the probe asks for a redraw on a click");
        assert_eq!(seen.borrow().last(), Some(&(100.0, 50.0, true)));
        ui.scroll_by(1); // two steps of ROW1
        ui.hover(Some((bx + 100.0, by + 50.0)), 1920.0, 1080.0);
        assert_eq!(seen.borrow().last(), Some(&(100.0, 50.0 + 2.0 * ROW1, false)), "the scroll is added to y");
        let n = seen.borrow().len();
        ui.hover(Some((bx - 5.0, by + 50.0)), 1920.0, 1080.0); // left of the body
        ui.hover(Some((bx + 100.0, by - 5.0)), 1920.0, 1080.0); // above it
        assert_eq!(seen.borrow().len(), n, "outside the body the page hears nothing");
        ui.go(0);
        let n = seen.borrow().len();
        ui.press(bx + 100.0, by + 50.0, 1920.0, 1080.0);
        assert_eq!(seen.borrow().len(), n, "a page that is not showing hears nothing");
    }

    #[test]
    fn only_the_page_being_shown_is_ticked() {
        let (mut ui, page, _, ticks) = ui_with_probe();
        assert!(!ui.tick() && ticks.get() == 0, "the keybinds page has no clock");
        ui.go(page);
        assert!(ui.tick() && ui.pending(), "a page that changed asks for a frame");
        assert_eq!(ticks.get(), 1);
        ui.type_str("x"); // the search results are showing, not the page
        assert!(!ui.tick() && ticks.get() == 1);
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

    /// A page of a given height, to try the scrollbar's edge.
    struct Fixed(f32);

    impl Page for Fixed {
        fn name(&self) -> &'static str {
            "fixed"
        }
        fn blurb(&self) -> String {
            String::new()
        }
        fn draw(&mut self, _: &mut Canvas, _: &mut Fonts, _: f32) -> f32 {
            self.0
        }
    }

    #[test]
    fn a_page_exactly_as_tall_as_its_body_has_nothing_to_scroll() {
        let mut ui = Ui::new();
        ui.fit(1920.0, 1080.0);
        let body = ui.body_h();
        for (height, scrolls) in [(body - 1.0, false), (body, false), (body + 1.0, true)] {
            ui.extra = vec![Box::new(Fixed(height))];
            ui.page_total = None;
            ui.go(1);
            let (total, visible) = ui.extent();
            assert_eq!(total > visible, scrolls, "a page {height} px tall in a body of {body}");
        }
    }

    #[test]
    fn scroll_stays_in_range_on_a_page_that_says_how_tall_it_is() {
        let (mut ui, page, _, _) = ui_with_probe();
        ui.fit(1280.0, 720.0);
        ui.go(page);
        let (total, visible) = ui.extent();
        assert_eq!(total, (3000.0 / ROW1).ceil() as usize, "the height the page returned");
        assert!(total > visible);
        ui.scroll_by(1000);
        assert_eq!(ui.scroll, total - visible);
        ui.scroll_by(-1000);
        assert_eq!(ui.scroll, 0);
    }
}
