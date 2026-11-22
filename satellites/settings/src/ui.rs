//! The settings page: state, layout and drawing, with no Wayland in it so `--dump`
//! and the tests can run it. Every setting lives in one of Seiza's small config
//! files (see `seiza_common::config`); the programs that care read their own.
//! Pages are stars on a thread across the top; each page is cards of rows, and typing searches them all.

use std::collections::HashMap;

use seiza_common::{
    config,
    draw::{shared_font, Canvas, Text},
    kit::{self, Thread, PILL_W},
    palette::{palette, rgba},
};

const PAD: f32 = 30.0; // between the card's edge and its content
const TOP: f32 = 168.0; // where a page's body starts, from the card's top
const FOOT: f32 = 60.0; // the strip under the body
const GAP: f32 = 18.0; // between cards
const MINCOL: f32 = 430.0; // the narrowest a column of cards may get
const ROW: f32 = 58.0; // a row with its control beside the words
const INNER: f32 = 18.0; // between a card's edge and its rows
const UNIT: f32 = 60.0; // one scroll step, in pixels

#[derive(Clone, Copy)]
pub enum Ctl {
    Slider(f32, f32, f32), // min, max, step
    Choice(&'static [&'static str]),
    Toggle,
    Path,
    Button(&'static str),
    Swatches, // the palette's colors, read-only
    None,     // no control: a documentation row, all words
}

pub struct Row {
    pub id: &'static str, // "file/key"
    pub label: &'static str,
    pub hint: &'static str,
    pub ctl: Ctl,
    pub default: &'static str,
}

pub struct Card {
    pub title: &'static str,
    pub rows: Vec<Row>,
}

pub struct Page {
    pub name: &'static str,
    pub blurb: &'static str,
    pub cards: Vec<Card>,
}

pub fn pages() -> Vec<Page> {
    let row = |id, label, hint, ctl, default| Row { id, label, hint, ctl, default };
    let tog = |id, label, hint, default| row(id, label, hint, Ctl::Toggle, default);
    let card = |title, rows| Card { title, rows };
    vec![
        Page { name: "sky", blurb: "seiza reads a picture and borrows its colors. the stars and the camera live here too.", cards: vec![
            card("colors", vec![
                row("look/swatches", "the sky now", "the colors every program draws with", Ctl::Swatches, ""),
                row("look/wallpaper", "colors from picture", "wallust reads it", Ctl::Path, ""),
                row("look/random", "surprise me", "one from the same folder", Ctl::Button("random"), ""),
                row("look/style", "palette style", "how boldly the picture colors the sky", Ctl::Choice(&["soft", "hard", "comp", "salient"]), "soft"),
                row("look/apply", "borrow the colors", "restarts the bar, the sky and the widgets", Ctl::Button("apply"), ""),
                tog("look/contrast_guard", "keep text readable", "swaps in cream text if a picture is too dim", "1"),
            ]),
            card("night", vec![
                row("sky/nebula", "painted sky", "how much cloud the night shows", Ctl::Slider(0.0, 1.0, 0.05), "0.4"),
                tog("sky/shooting", "shooting stars", "now and then", "1"),
                tog("sky/labels", "star names", "in the overview", "1"),
                tog("sky/threads", "threads", "the lines joining stars in the overview", "1"),
            ]),
            card("motion", vec![
                row("motion/pan_secs", "pan duration", "how long a pan between altitudes takes", Ctl::Slider(0.1, 2.0, 0.05), "0.35"),
                row("motion/easing", "easing", "how a pan settles", Ctl::Choice(&["drift", "glide", "float", "calm", "bounce"]), "drift"),
                tog("motion/reduce", "reduce motion", "pans become jumps", "0"),
            ]),
        ]},
        Page { name: "windows", blurb: "the frame around every window.", cards: vec![
            card("frame", vec![
                row("frame/radius", "corner roundness", "in pixels", Ctl::Slider(0.0, 32.0, 1.0), "14"),
                row("frame/border", "frame thickness", "in pixels", Ctl::Slider(0.0, 8.0, 1.0), "2"),
                row("frame/glow", "glow", "the bloom behind a window", Ctl::Choice(&["off", "soft", "bright"]), "soft"),
            ]),
        ]},
        Page { name: "bar", blurb: "what the bar says.", cards: vec![
            card("time", vec![
                row("look/clock", "clock", "24 hours or 12", Ctl::Choice(&["24", "12"]), "24"),
                tog("bar/date", "date", "next to the clock", "1"),
            ]),
            card("system", vec![
                tog("bar/stats", "system stats", "cpu and ram", "1"),
                tog("bar/volume", "volume", "the speaker level", "1"),
                tog("bar/battery", "battery", "only where there is one", "1"),
                tog("bar/net", "network", "network and bluetooth", "1"),
            ]),
            card("extras", vec![
                tog("bar/dots", "star dots", "one per star; click to fly there", "1"),
                tog("bar/media", "now playing", "click to play or pause", "1"),
                tog("bar/quick", "quick buttons", "screenshot and microphone", "1"),
                tog("bar/weather", "weather", "the temperature (asks wttr.in)", "0"),
            ]),
        ]},
        Page { name: "cards", blurb: "what lives on the sky, and in the side panel.", cards: vec![
            card("on the sky", vec![
                tog("widgets/clock", "clock", "the time, and the moon's phase", "1"),
                tog("widgets/calendar", "calendar", "this month", "1"),
                tog("widgets/system", "system", "cpu, ram and temperature", "1"),
                tog("widgets/media", "media", "what is playing, from any MPRIS player", "1"),
                tog("widgets/weather", "weather", "from wttr.in; set weather.place in seiza.kdl", "0"),
                tog("widgets/agenda", "agenda", "next events; set calendar.source in seiza.kdl", "0"),
            ]),
            card("side panel", vec![
                tog("panel/weather", "weather", "a card with the temperature (asks wttr.in)", "0"),
                tog("panel/calendar", "calendar", "this month, today lit", "0"),
            ]),
        ]},
        Page { name: "yoru", blurb: "how the launcher and the menus behave.", cards: vec![
            card("yoru", vec![
                tog("yoru/lines", "Yoru's lines", "the little quips on the lock screen and launcher", "1"),
                tog("yoru/confirm", "confirm power actions", "press twice to shut down or log out", "1"),
                row("ysh/voice", "ysh voice", "how much Yoru says in the shell: nothing, hi and bye, or everything", Ctl::Choice(&["off", "quiet", "full"]), "full"),
            ]),
            card("launcher", vec![
                row("launcher/style", "launcher look", "a list, or apps as stars", Ctl::Choice(&["list", "stars"]), "list"),
                row("launcher/search", "web search", "what a query that is no app asks", Ctl::Choice(&["duckduckgo", "google", "startpage", "brave"]), "duckduckgo"),
                row("launcher/terminal", "terminal", "runs apps that want one; kitty if empty", Ctl::Path, ""),
            ]),
        ]},
        Page { name: "keys", blurb: "keys are written in ~/.config/seiza/seiza.kdl.", cards: vec![
            card("bindings", vec![
                row("keys/help", "show all bindings", "asks seiza to list them", Ctl::Button("show"), ""),
                row("keys/reload", "reload keys", "after editing the file", Ctl::Button("reload"), ""),
            ]),
        ]},
        docs_page(),
    ]
}

/// The documentation page: the commands, the words Seiza uses, config recipes, every option and every file,
/// as cards of rows with no control (they are found by the search like any setting).
/// shortcut: the titles and ids are built at run time and leaked, once, because `Row` holds `&'static str`;
/// this program lives for one visit, so that is a few kilobytes until it closes.
fn docs_page() -> Page {
    use seiza_common::docs;
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let n = std::cell::Cell::new(0); // numbers the rows across the whole page, so no two share an id
    let row = |label: &'static str, hint: &'static str| {
        n.set(n.get() + 1);
        Row { id: leak(format!("doc/{}", n.get())), label, hint, ctl: Ctl::None, default: "" }
    };
    let mut cards = vec![];
    for (kind, set) in [("commands", docs::COMMANDS), ("guide", docs::GUIDE), ("recipes", docs::RECIPES)] {
        for (title, rows) in set {
            cards.push(Card { title: leak(format!("{kind} \u{b7} {title}")), rows: rows.iter().map(|(a, b)| row(a, b)).collect() });
        }
    }
    for (section, rows) in docs::options_cards() {
        cards.push(Card { title: leak(format!("options \u{b7} {section}")), rows: rows.into_iter().map(|(a, b)| row(leak(a), leak(b))).collect() });
    }
    for (title, rows) in docs::FILES {
        cards.push(Card { title: leak(format!("files \u{b7} {title}")), rows: rows.iter().map(|(a, b)| row(a, b)).collect() });
    }
    Page { name: "documentation", blurb: "how Seiza works: the commands, the words it uses, recipes for the config file, every option and every file.", cards }
}

/// ysh reads YSH_VOICE from its own config file, so that is where the setting lives.
fn ysh_rc() -> std::path::PathBuf {
    let dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    std::path::PathBuf::from(dir).join("ysh/config.ysh")
}

/// The value of the `export YSH_VOICE=` line, if the file has one.
fn ysh_voice(text: &str) -> Option<&str> {
    text.lines().find_map(|l| l.trim().strip_prefix("export YSH_VOICE=")).map(|v| v.trim_matches(['"', '\'']))
}

/// `text` with its YSH_VOICE line set to `val` (added at the end if missing); every other line stays.
fn with_voice(text: &str, val: &str) -> String {
    let line = format!("export YSH_VOICE={val}");
    let mut done = false;
    let mut out: Vec<String> = text.lines().map(|l| if l.trim().starts_with("export YSH_VOICE=") { done = true; line.clone() } else { l.to_string() }).collect();
    if !done {
        out.push(line);
    }
    out.join("\n") + "\n"
}

const WIDGET_NAMES: [&str; 6] = ["clock", "calendar", "system", "media", "weather", "agenda"];

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

type Rect = (f32, f32, f32, f32); // x, y, w, h

/// Where a row's first line of hint sits: under its label, or at the label's place if it has none.
fn hint_y(r: &Row) -> f32 {
    if r.label.is_empty() { 25.0 } else { 44.0 }
}

/// One row, placed: where it is and where its control is, relative to the page body.
struct Placed {
    id: &'static str,
    ctl: Ctl,
    label: &'static str,
    lines: Vec<String>, // the hint, wrapped
    hint_y: f32,        // where its first line sits, from the row's top
    row: Rect,
    ctl_box: Rect,
    first: bool,
    thumb: bool, // a picture preview sits under the words
}

struct Layout {
    cards: Vec<(String, Rect)>,
    rows: Vec<Placed>,
    total: f32, // the tallest column, scrolled or not
}

/// The cards to show: the page's own, or, while searching, the rows of every page that match all the words
/// typed (one card per card that has any).
fn view<'a>(pages: &'a [Page], page: usize, query: &str) -> Vec<(String, Vec<&'a Row>)> {
    if query.trim().is_empty() {
        return pages[page].cards.iter().map(|c| (c.title.to_string(), c.rows.iter().collect())).collect();
    }
    let words: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
    let mut out = vec![];
    for p in pages {
        for c in &p.cards {
            let rows: Vec<&Row> = c
                .rows
                .iter()
                .filter(|r| {
                    let hay = format!("{} {} {} {}", p.name, c.title, r.label, r.hint).to_lowercase();
                    words.iter().all(|w| hay.contains(w.as_str()))
                })
                .collect();
            if !rows.is_empty() {
                out.push((format!("{} \u{b7} {}", p.name, c.title), rows));
            }
        }
    }
    out
}

/// How big a control is (a slider's width is whatever its row leaves).
fn ctl_size(ctl: Ctl, t13: &mut Text) -> (f32, f32) {
    match ctl {
        Ctl::Slider(..) => (0.0, 24.0),
        Ctl::Choice(opts) => (opts.iter().map(|o| t13.width(o) + 30.0).sum::<f32>() + 8.0, 34.0),
        Ctl::Toggle => (50.0, 28.0),
        Ctl::Path => (300.0, 36.0),
        Ctl::Button(t) => ((t13.width(t) + 44.0).max(90.0), 34.0),
        Ctl::Swatches => (6.0 * 30.0, 24.0),
        Ctl::None => (0.0, 0.0),
    }
}

/// Deal `view` into columns (as many as are wide enough, in order, each card into the shortest) and place
/// every row, scrolled by `scroll` pixels. A control sits beside its words; sliders and wide choices go under.
fn layout(view: &[(String, Vec<&Row>)], t15: &mut Text, t13: &mut Text, thumb: bool, cw: f32, scroll: f32) -> Layout {
    let cols = if cw >= 2.0 * MINCOL + GAP { 2 } else { 1 };
    let col_w = (cw - GAP * (cols - 1) as f32) / cols as f32;
    let inner = col_w - 2.0 * INNER;
    // per row: (control under the words, its height, control size, hint lines)
    let shapes: Vec<Vec<(bool, f32, (f32, f32), Vec<String>)>> = view
        .iter()
        .map(|(_, rows)| {
            rows.iter()
                .map(|r| {
                    let (mut w, h) = ctl_size(r.ctl, t13);
                    let label_w = t15.width(r.label);
                    if matches!(r.ctl, Ctl::Path) {
                        w = w.min(inner - label_w - 24.0).max(120.0); // a text field gives way to its label
                    }
                    let crowded = !matches!(r.ctl, Ctl::Path) && label_w + w + 24.0 > inner;
                    let under = matches!(r.ctl, Ctl::Slider(..)) || (matches!(r.ctl, Ctl::Choice(_)) && w > inner * 0.35) || crowded;
                    let words_only = matches!(r.ctl, Ctl::None);
                    let lines = kit::wrap(t13, r.hint, if under || words_only { inner } else { inner - w - 16.0 });
                    let mut rh = if under { 58.0 + h + 12.0 } else { ROW } + 16.0 * (lines.len() - 1) as f32;
                    if words_only {
                        rh = hint_y(r) + 16.0 * (lines.len() - 1) as f32 + 14.0; // the words, and a little air under them
                    }
                    if thumb && r.id == "look/wallpaper" {
                        rh += 62.0;
                    }
                    (under, rh, (w, h), lines)
                })
                .collect()
        })
        .collect();
    let heights: Vec<f32> = shapes.iter().map(|rows| kit::CARD_HEAD + rows.iter().map(|s| s.1).sum::<f32>() + 4.0).collect();
    let mut used = vec![0.0f32; cols];
    let mut out = Layout { cards: vec![], rows: vec![], total: 0.0 };
    for (i, (title, rows)) in view.iter().enumerate() {
        let c = (0..cols).min_by(|&a, &b| used[a].total_cmp(&used[b])).unwrap_or(0);
        let (x, top) = (c as f32 * (col_w + GAP), used[c] - scroll);
        used[c] += heights[i] + GAP;
        out.cards.push((title.clone(), (x, top, col_w, heights[i])));
        let mut ry = top + kit::CARD_HEAD;
        for (k, r) in rows.iter().enumerate() {
            let (under, rh, (w, h), lines) = shapes[i][k].clone();
            let ctl_box = if matches!(r.ctl, Ctl::Slider(..)) {
                (x + INNER + 48.0, ry + 58.0, inner - 48.0, h)
            } else if under {
                (x + INNER, ry + 58.0, w, h)
            } else {
                (x + col_w - INNER - w, ry + (ROW - h) / 2.0, w, h)
            };
            out.rows.push(Placed { id: r.id, ctl: r.ctl, label: r.label, lines, hint_y: hint_y(r), row: (x, ry, col_w, rh), ctl_box, first: k == 0, thumb: thumb && r.id == "look/wallpaper" });
            ry += rh;
        }
    }
    out.total = used.iter().cloned().fold(0.0, f32::max);
    out
}

pub struct Ui {
    pages: Vec<Page>,
    pub page: usize,
    scroll: f32,       // pixels scrolled off the top of the body
    query: String,     // what is typed in the search pill
    pub values: HashMap<&'static str, String>,
    pub focus: Option<&'static str>, // the text field being typed into, or the slider being dragged
    pub msg: String,
    pub restart: Vec<&'static str>, // programs to restart when the page closes
    thumb: Option<(usize, usize, Vec<u32>)>,
    hover: Option<usize>, // the page star under the pointer
    glide: kit::Glide,    // the big star moving to another page
    stale: bool,          // what is in `content` is out of date
    content: Option<Canvas>,
    back: Option<Canvas>, // the card's plate
    body: Option<Canvas>, // the page's body
    body_stale: bool,
    w: f32, // the card's size: most of the screen
    h: f32,
    t22: Text,
    t15: Text,
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
        for r in pages.iter().flat_map(|p| p.cards.iter().flat_map(|c| &c.rows)).filter(|r| !matches!(r.ctl, Ctl::None)) {
            let v = match (r.id.split_once('/'), &widgets) {
                (Some(("widgets", name)), Some(text)) => if text.lines().any(|l| l.trim() == name) { "1" } else { "0" }.to_string(),
                (Some(("ysh", _)), _) => ysh_voice(&std::fs::read_to_string(ysh_rc()).unwrap_or_default()).unwrap_or(r.default).to_string(),
                (Some((file, key)), _) if file != "widgets" && matches!(r.ctl, Ctl::Toggle) => if config::flag(file, key, r.default == "1") { "1" } else { "0" }.to_string(),
                (Some((file, key)), _) if file != "widgets" => config::get(file, key).unwrap_or_else(|| r.default.to_string()),
                _ => r.default.to_string(),
            };
            values.insert(r.id, v);
        }
        let font = shared_font();
        let mut ui = Ui {
            pages,
            page: 0,
            scroll: 0.0,
            query: String::new(),
            values,
            focus: None,
            msg: "most apply at once  \u{b7}  the sky and the cards when this closes".into(),
            restart: vec![],
            thumb: None,
            hover: None,
            glide: kit::Glide::default(),
            stale: true,
            content: None,
            back: None,
            body: None,
            body_stale: true,
            w: 1200.0,
            h: 740.0,
            t22: Text::new(font.clone(), 24.0),
            t15: Text::new(font.clone(), 15.0),
            t13: Text::new(font, 13.0),
        };
        ui.load_thumb();
        ui
    }

    pub fn load_thumb(&mut self) {
        self.thumb = Some(self.values["look/wallpaper"].as_str()).filter(|p| !p.trim().is_empty()).and_then(thumbnail);
        self.stale = true;
        self.body_stale = true;
    }

    fn cw(&self) -> f32 {
        self.w - 2.0 * PAD
    }

    fn body_h(&self) -> f32 {
        self.h - TOP - FOOT
    }

    /// Size the card to the screen: most of it, so the desktop still shows around it.
    fn fit(&mut self, w: f32, h: f32) {
        self.w = (w * 0.78).clamp(1000.0, 1500.0).min(w - 40.0);
        self.h = (h * 0.82).clamp(640.0, 880.0).min(h - 40.0);
    }

    /// The card sits a little below the middle, under the bar.
    fn origin(&self, w: f32, h: f32) -> (f32, f32) {
        (((w - self.w) / 2.0).round(), ((h - self.h) / 2.0 + 18.0).round())
    }

    /// The row of page stars across the card's top.
    fn thread(&self, ox: f32, oy: f32) -> Thread {
        Thread::new(ox + 200.0, ox + self.w - PAD - PILL_W - 30.0, oy + 40.0, self.pages.len())
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn searching(&self) -> bool {
        !self.query.trim().is_empty()
    }

    fn lay(&mut self) -> Layout {
        let (cw, thumb, scroll) = (self.cw(), self.thumb.is_some(), self.scroll);
        let view = view(&self.pages, self.page, &self.query);
        layout(&view, &mut self.t15, &mut self.t13, thumb, cw, scroll)
    }

    /// Scroll by `d` steps (wheel, arrows, PageUp/PageDown); a page that fits stays put.
    pub fn scroll_by(&mut self, d: f32) {
        let max = (self.lay().total - self.body_h()).max(0.0);
        self.scroll = (self.scroll + d * UNIT).clamp(0.0, max);
        self.stale = true;
        self.body_stale = true;
    }

    pub fn go(&mut self, page: usize) {
        let page = page.min(self.pages.len() - 1);
        if page != self.page {
            self.glide.start(self.glide.pos(self.page));
        }
        self.page = page;
        self.query.clear();
        self.scroll = 0.0;
        self.stale = true;
        self.body_stale = true;
    }

    /// True while a frame is still owed: the star is moving, or what is shown is out of date.
    pub fn pending(&self) -> bool {
        self.stale
    }

    /// The pointer moved to `at` (None: it left); true if the highlight changed and a redraw is due.
    pub fn hover(&mut self, at: Option<(f32, f32)>, w: f32, h: f32) -> bool {
        self.fit(w, h);
        let (ox, oy) = self.origin(w, h);
        let now = at.and_then(|(x, y)| self.thread(ox, oy).hit(x, y));
        let changed = std::mem::replace(&mut self.hover, now) != now;
        self.stale |= changed;
        changed
    }

    pub fn set(&mut self, id: &'static str, val: String) -> Effect {
        self.values.insert(id, val.clone());
        self.stale = true;
        self.body_stale = true;
        let (file, key) = id.split_once('/').unwrap();
        let saved = match file {
            "widgets" => {
                let text = widgets_file(|n| self.values.get(format!("widgets/{n}").as_str()).is_some_and(|v| v == "1"));
                std::fs::create_dir_all(config::path("widgets").parent().unwrap()).and_then(|_| std::fs::write(config::path("widgets"), text))
            }
            "look" if matches!(key, "apply" | "random") => Ok(()),
            "keys" => Ok(()),
            "ysh" => {
                let rc = ysh_rc();
                std::fs::write(&rc, with_voice(&std::fs::read_to_string(&rc).unwrap_or_default(), &val))
            }
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

    /// A slider's value for a pointer at body x `bx`.
    fn slide(&mut self, p: &Placed, bx: f32) -> Effect {
        let Ctl::Slider(min, max, step) = p.ctl else { return Effect::None };
        let (cx, _, cw, _) = p.ctl_box;
        let t = ((bx - cx) / cw).clamp(0.0, 1.0);
        let v = ((min + (max - min) * t) / step).round() * step;
        self.set(p.id, if step.fract() == 0.0 { format!("{v}") } else { format!("{v:.2}") })
    }

    /// A press (or drag) at a screen point.
    pub fn press(&mut self, x: f32, y: f32, w: f32, h: f32, dragging: bool) -> Effect {
        self.fit(w, h);
        let (ox, oy) = self.origin(w, h);
        if dragging {
            // a slider stays under the pointer even when the pointer leaves it
            let Some(id) = self.focus else { return Effect::None };
            let lay = self.lay();
            return match lay.rows.iter().find(|p| p.id == id) {
                Some(p) => self.slide(p, x - ox - PAD),
                None => Effect::None,
            };
        }
        self.focus = None;
        if let Some(i) = self.thread(ox, oy).hit(x, y) {
            self.go(i);
            return Effect::None;
        }
        let (bx, by) = (x - ox - PAD, y - oy - TOP);
        if by < 0.0 || by > self.body_h() {
            return Effect::None;
        }
        let lay = self.lay();
        for p in lay.rows.iter().filter(|p| !matches!(p.ctl, Ctl::None)) {
            let (cx, cy, cw, ch) = p.ctl_box;
            if !(bx >= cx - 6.0 && bx < cx + cw + 6.0 && by >= cy - 6.0 && by < cy + ch + 6.0) {
                continue;
            }
            return match p.ctl {
                Ctl::Slider(..) => {
                    self.focus = Some(p.id); // dragging stays on this slider
                    self.slide(p, bx)
                }
                Ctl::Choice(opts) => {
                    let mut sx = cx + 4.0;
                    let mut pick = opts[opts.len() - 1];
                    for o in opts {
                        let sw = self.t13.width(o) + 30.0;
                        if bx < sx + sw {
                            pick = o;
                            break;
                        }
                        sx += sw;
                    }
                    self.set(p.id, pick.to_string())
                }
                Ctl::Toggle => {
                    let on = self.values[p.id] == "1";
                    self.set(p.id, if on { "0" } else { "1" }.into())
                }
                Ctl::Path => {
                    self.focus = Some(p.id);
                    self.stale = true;
                    self.body_stale = true;
                    Effect::None
                }
                Ctl::Button(_) => self.set(p.id, "1".into()),
                Ctl::Swatches | Ctl::None => Effect::None,
            };
        }
        Effect::None
    }

    pub fn release(&mut self) {
        let slider = |id: &str| self.pages.iter().flat_map(|p| p.cards.iter().flat_map(|c| &c.rows)).any(|r| r.id == id && matches!(r.ctl, Ctl::Slider(..)));
        if self.focus.is_some_and(slider) {
            self.focus = None;
        }
    }

    /// Typing goes into the focused text field, else into the search.
    pub fn type_text(&mut self, s: &str) {
        if let Some(id) = self.focus {
            self.values.get_mut(id).unwrap().push_str(s);
            let v = self.values[id].clone();
            self.set(id, v);
        } else {
            self.query.push_str(s);
            self.query_changed();
        }
    }

    pub fn backspace(&mut self) {
        if let Some(id) = self.focus {
            self.values.get_mut(id).unwrap().pop();
            let v = self.values[id].clone();
            self.set(id, v);
        } else if self.query.pop().is_some() {
            self.query_changed();
        }
    }

    fn query_changed(&mut self) {
        self.scroll = 0.0;
        self.stale = true;
        self.body_stale = true;
    }

    /// Esc: leave the text field, else empty the search. False if there was nothing to leave: the caller closes.
    pub fn escape(&mut self) -> bool {
        let had = self.focus.take().is_some() || !self.query.is_empty();
        self.query.clear();
        if had {
            self.query_changed();
        }
        had
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

    fn draw_card(&mut self, w: usize, h: usize) -> Canvas {
        let p = palette();
        let (ox, oy) = self.origin(w as f32, h as f32);
        if self.back.as_ref().is_none_or(|b| b.w != w || b.h != h) {
            let mut b = kit::window_plate(w, h, (ox, oy), (self.w, self.h), FOOT);
            self.t22.draw(&mut b, ox + PAD, oy + 46.0, "settings", rgba(p.text, 0.95));
            self.back = Some(b);
        }
        let mut cv = self.back.clone().expect("made above");
        let names: Vec<&str> = self.pages.iter().map(|p| p.name).collect();
        let (pos, hover) = (if self.searching() { -9.0 } else { self.glide.pos(self.page) }, self.hover); // no big star while searching
        self.thread(ox, oy).draw(&mut cv, &mut self.t13, &names, pos, hover);

        let (title, blurb) = if self.searching() {
            let n: usize = view(&self.pages, self.page, &self.query).iter().map(|(_, r)| r.len()).sum();
            let found = if n == 0 { "nothing matches that".to_string() } else { format!("{n} result{} across every page", if n == 1 { "" } else { "s" }) };
            ("search", found)
        } else {
            (self.pages[self.page].name, self.pages[self.page].blurb.to_string())
        };
        self.t22.draw(&mut cv, ox + PAD, oy + 122.0, title, rgba(p.text, 1.0));
        self.t13.draw(&mut cv, ox + PAD, oy + 146.0, &blurb, rgba(p.text, 0.55));
        kit::search_pill(&mut cv, &mut self.t15, ox + self.w - PAD - PILL_W, oy + 23.0, &self.query, "find a setting..");

        let (cw, bh) = (self.cw() as usize, self.body_h() as usize);
        if self.body_stale || self.body.as_ref().is_none_or(|b| b.w != cw || b.h != bh) {
            self.body = Some(self.draw_body());
            self.body_stale = false;
        }
        cv.over(self.body.as_ref().expect("made above"), (ox + PAD) as i32, (oy + TOP) as i32);

        let total = self.lay().total;
        let more = self.scroll + 1.0 < total - bh as f32;
        if total > bh as f32 {
            let (track, frac) = (bh as f32, bh as f32 / total);
            cv.round_rect(ox + self.w - 14.0, oy + TOP, 4.0, track, 2.0, rgba(p.text, 0.1));
            cv.round_rect(ox + self.w - 14.0, oy + TOP + track * self.scroll / total, 4.0, track * frac, 2.0, rgba(p.star, 0.9));
        }
        let base = oy + self.h - 22.0;
        self.t13.draw(&mut cv, ox + PAD, base, &self.msg, rgba(p.text, 0.55));
        let esc = "esc to close";
        let ew = self.t13.width(esc);
        self.t13.draw(&mut cv, ox + self.w - PAD - ew, base, esc, rgba(p.text, 0.35));
        if more {
            let more_txt = "scroll for more";
            let mw = self.t13.width(more_txt);
            self.t13.draw(&mut cv, ox + self.w - PAD - ew - 24.0 - mw, base, more_txt, rgba(p.star, 0.8));
        }
        cv
    }

    /// The page's cards and their rows, at the body's own origin.
    fn draw_body(&mut self) -> Canvas {
        let p = palette();
        let (cw, bh) = (self.cw() as usize, self.body_h() as usize);
        let mut cv = Canvas { w: cw, h: bh, px: vec![0; cw * bh] };
        let lay = self.lay();
        for (title, (x, y, w, h)) in &lay.cards {
            if y + h >= 0.0 && *y <= bh as f32 {
                kit::card(&mut cv, &mut self.t13, *x, *y, *w, *h, title);
            }
        }
        for r in &lay.rows {
            let (x, y, w, h) = r.row;
            if y + h < 0.0 || y > bh as f32 {
                continue;
            }
            if !r.first {
                cv.round_rect(x + 1.0, y, w - 2.0, 1.0, 0.0, rgba(p.text, 0.06));
            }
            let words_only = matches!(r.ctl, Ctl::None);
            self.t15.draw(&mut cv, x + INNER, y + 25.0, r.label, if words_only { rgba(p.star, 0.9) } else { rgba(p.text, 0.95) });
            for (i, line) in r.lines.iter().enumerate() {
                self.t13.draw(&mut cv, x + INNER, y + r.hint_y + 16.0 * i as f32, line, rgba(p.text, if words_only { 0.72 } else { 0.5 }));
            }
            if let (true, Some((tw, th, px))) = (r.thumb, &self.thumb) {
                let (tx, ty) = ((x + INNER) as i32, (y + 56.0 + 16.0 * (r.lines.len() - 1) as f32) as i32);
                cv.round_rect(tx as f32 - 1.0, ty as f32 - 1.0, *tw as f32 + 2.0, *th as f32 + 2.0, 3.0, rgba(p.star, 0.3));
                for yy in 0..*th {
                    let py = ty + yy as i32;
                    if py >= 0 && (py as usize) < cv.h && tx >= 0 && tx as usize + tw <= cv.w {
                        cv.px[py as usize * cv.w + tx as usize..py as usize * cv.w + tx as usize + tw].copy_from_slice(&px[yy * tw..(yy + 1) * tw]);
                    }
                }
            }
            self.draw_ctl(&mut cv, r);
        }
        cv
    }

    fn draw_ctl(&mut self, cv: &mut Canvas, r: &Placed) {
        if matches!(r.ctl, Ctl::None) {
            return;
        }
        let p = palette();
        let (cx, cy, cw, ch) = r.ctl_box;
        let val = self.values[r.id].clone();
        match r.ctl {
            Ctl::Slider(min, max, _) => {
                let t = ((val.parse::<f32>().unwrap_or(min) - min) / (max - min)).clamp(0.0, 1.0);
                let my = cy + ch / 2.0;
                self.t13.draw(cv, r.row.0 + INNER, my + 5.0, &val, rgba(p.text, 0.7));
                cv.round_rect(cx, my - 2.0, cw, 4.0, 2.0, rgba(p.text, 0.12));
                cv.round_rect(cx, my - 2.0, (cw * t).max(4.0), 4.0, 2.0, rgba(p.star, 0.9));
                let kx = cx + cw * t;
                cv.round_rect(kx - 10.0, my - 10.0, 20.0, 20.0, 10.0, rgba(p.star, 0.25));
                cv.round_rect(kx - 7.5, my - 7.5, 15.0, 15.0, 7.5, rgba(p.text, 0.96));
            }
            Ctl::Choice(opts) => {
                cv.round_rect(cx, cy, cw, ch, ch / 2.0, rgba(p.text, 0.14));
                cv.round_rect(cx + 1.0, cy + 1.0, cw - 2.0, ch - 2.0, ch / 2.0 - 1.0, rgba(p.sky, 1.0));
                let mut sx = cx + 4.0;
                for o in opts {
                    let sw = self.t13.width(o) + 30.0;
                    let on = *o == val;
                    if on {
                        cv.round_rect(sx, cy + 3.0, sw, ch - 6.0, (ch - 6.0) / 2.0, rgba(p.dust, 1.0));
                    }
                    self.t13.draw(cv, sx + 15.0, cy + ch / 2.0 + 5.0, o, rgba(p.text, if on { 1.0 } else { 0.7 }));
                    sx += sw;
                }
            }
            Ctl::Toggle => {
                let on = val == "1";
                cv.round_rect(cx, cy, cw, ch, ch / 2.0, rgba(if on { p.star } else { p.text }, if on { 0.75 } else { 0.2 }));
                cv.round_rect(cx + 1.0, cy + 1.0, cw - 2.0, ch - 2.0, ch / 2.0 - 1.0, rgba(if on { p.dust } else { p.sky }, 1.0));
                if on {
                    cv.sparkle(cx + cw - ch / 2.0, cy + ch / 2.0, 8.0, rgba(p.star, 1.0));
                } else {
                    cv.round_rect(cx + ch / 2.0 - 5.0, cy + ch / 2.0 - 5.0, 10.0, 10.0, 5.0, rgba(p.text, 0.5));
                }
            }
            Ctl::Path => {
                let focused = self.focus == Some(r.id);
                cv.round_rect(cx, cy, cw, ch, ch / 2.0, rgba(if focused { p.star } else { p.text }, if focused { 0.7 } else { 0.15 }));
                cv.round_rect(cx + 1.0, cy + 1.0, cw - 2.0, ch - 2.0, ch / 2.0 - 1.0, rgba(p.sky, 1.0));
                let hint = val.is_empty() && !focused;
                let shown = if hint { (if r.id == "look/wallpaper" { "~/pictures/sky.jpg" } else { "kitty" }).to_string() } else { val.clone() };
                // keep the tail of a long path in view
                let tail: String = shown.chars().rev().scan(0.0, |acc, c| { *acc += self.t13.glyph(c).0.advance_width; (*acc < cw - 36.0).then_some(c) }).collect::<Vec<_>>().into_iter().rev().collect();
                let end = self.t13.draw(cv, cx + 18.0, cy + ch / 2.0 + 5.0, &tail, rgba(p.text, if hint { 0.3 } else { 1.0 }));
                if focused {
                    cv.round_rect(end + 1.0, cy + 9.0, 1.5, ch - 18.0, 0.0, rgba(p.text, 0.9));
                }
            }
            Ctl::Swatches => {
                for (n, c) in [p.sky, p.dust, p.mist, p.glow, p.star, p.text].into_iter().enumerate() {
                    let x = cx + n as f32 * 30.0;
                    cv.round_rect(x, cy + 1.0, 24.0, 24.0, 12.0, rgba(p.text, 0.25));
                    cv.round_rect(x + 1.0, cy + 2.0, 22.0, 22.0, 11.0, rgba(c, 1.0));
                }
            }
            Ctl::None => {}
            Ctl::Button(text) => {
                cv.round_rect(cx, cy, cw, ch, ch / 2.0, rgba(p.star, 0.85));
                cv.round_rect(cx + 1.0, cy + 1.0, cw - 2.0, ch - 2.0, ch / 2.0 - 1.0, rgba(p.panel, 1.0));
                let tw = self.t13.width(text);
                self.t13.draw(cv, cx + cw / 2.0 - tw / 2.0, cy + ch / 2.0 + 5.0, text, rgba(p.star, 1.0));
            }
        }
    }
}

#[cfg(test)]
impl Ui {
    /// The control of row `id` in screen coordinates, with the card fitted to a 1920x1080 screen.
    fn control_at(&mut self, id: &str) -> Rect {
        self.fit(1920.0, 1080.0);
        let (ox, oy) = self.origin(1920.0, 1080.0);
        let lay = self.lay();
        let p = lay.rows.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("{id} is not on this page"));
        let (x, y, w, h) = p.ctl_box;
        (ox + PAD + x, oy + TOP + y, w, h)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ysh_voice_line_is_read_and_replaced_in_place() {
        let t = "alias g='git'\nexport YSH_VOICE=quiet\n# end\n";
        assert_eq!(super::ysh_voice(t), Some("quiet"));
        assert_eq!(super::with_voice(t, "off"), "alias g='git'\nexport YSH_VOICE=off\n# end\n");
        assert_eq!(super::with_voice("alias a=b", "full"), "alias a=b\nexport YSH_VOICE=full\n");
        assert_eq!(super::ysh_voice("alias a=b"), None);
    }

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

    const SCREEN: (f32, f32) = (1920.0, 1080.0);

    #[test]
    fn clicking_controls_saves_to_the_files_the_programs_read() {
        with_config(|| {
            let mut ui = Ui::new();
            assert_eq!(ui.values["motion/easing"], "drift");
            let (w, h) = SCREEN;
            // the easing choice: click inside its second segment
            let (cx, cy, _, ch) = ui.control_at("motion/easing");
            let second = cx + 4.0 + ui.t13.width("drift") + 30.0 + 15.0;
            assert!(matches!(ui.press(second, cy + ch / 2.0, w, h, false), Effect::ReloadConfig));
            assert_eq!(config::get("motion", "easing").as_deref(), Some("glide"));
            // the reduce toggle
            let (cx, cy, cw, ch) = ui.control_at("motion/reduce");
            ui.press(cx + cw / 2.0, cy + ch / 2.0, w, h, false);
            assert_eq!(config::get("motion", "reduce").as_deref(), Some("1"));
            // a slider: click its far end
            let (cx, cy, cw, ch) = ui.control_at("motion/pan_secs");
            ui.press(cx + cw, cy + ch / 2.0, w, h, false);
            ui.release();
            assert_eq!(config::get("motion", "pan_secs").as_deref(), Some("2.00"));
            // the widgets toggles rewrite the whole file, and ask for a restart
            ui.go(3);
            let (cx, cy, cw, ch) = ui.control_at("widgets/calendar"); // calendar off
            ui.press(cx + cw / 2.0, cy + ch / 2.0, w, h, false);
            assert_eq!(std::fs::read_to_string(config::path("widgets")).unwrap(), "clock\nsystem\nmedia\n");
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
            ui.go(1);
            let (w, h) = SCREEN;
            let (cx, cy, cw, ch) = ui.control_at("frame/radius");
            ui.press(cx + cw, cy + ch / 2.0, w, h, false);
            assert_eq!(config::get("frame", "radius").as_deref(), Some("32"), "no .00 for the C parser");
            assert!(matches!(ui.set("keys/help", "1".into()), Effect::Run("seiza help")));
            assert!(!config::path("keys").exists());
        });
    }

    #[test]
    fn dragging_a_slider_follows_the_pointer_even_outside_it() {
        with_config(|| {
            let mut ui = Ui::new();
            ui.go(1);
            let (w, h) = SCREEN;
            let (cx, cy, cw, ch) = ui.control_at("frame/radius");
            ui.press(cx, cy + ch / 2.0, w, h, false);
            assert_eq!(config::get("frame", "radius").as_deref(), Some("0"));
            ui.press(cx + cw / 2.0, cy + 400.0, w, h, true); // far below the slider
            assert_eq!(config::get("frame", "radius").as_deref(), Some("16"));
            ui.release();
            assert!(ui.focus.is_none());
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
    fn typing_goes_to_the_focused_field_else_to_the_search() {
        with_config(|| {
            let mut ui = Ui::new();
            ui.type_text("x");
            assert_eq!(ui.values["look/wallpaper"], "", "no field focused: it is a search");
            assert!(ui.searching());
            assert!(ui.escape() && !ui.searching());
            ui.focus = Some("look/wallpaper");
            ui.type_text("/a");
            ui.type_text("b");
            ui.backspace();
            assert_eq!(ui.values["look/wallpaper"], "/a");
            assert!(ui.escape() && ui.focus.is_none(), "Esc leaves the field first");
            assert!(!ui.escape(), "and then there is nothing left: the caller closes");
        });
    }

    #[test]
    fn typing_finds_settings_on_every_page_and_they_still_work() {
        with_config(|| {
            let mut ui = Ui::new();
            ui.type_text("glow");
            let found = view(&ui.pages, ui.page, &ui.query);
            assert!(found.iter().any(|(t, rows)| t == "windows \u{b7} frame" && rows.iter().any(|r| r.id == "frame/glow")));
            let (w, h) = SCREEN;
            let (cx, cy, _, ch) = ui.control_at("frame/glow");
            ui.press(cx + 20.0, cy + ch / 2.0, w, h, false);
            assert_eq!(config::get("frame", "glow").as_deref(), Some("off"), "a found control is live");
            ui.type_text(" zzzz");
            assert!(view(&ui.pages, ui.page, &ui.query).is_empty());
        });
    }

    #[test]
    fn the_documentation_page_is_all_words_and_found_by_the_search() {
        with_config(|| {
            let mut ui = Ui::new();
            let docs = ui.pages.iter().position(|p| p.name == "documentation").expect("a documentation page");
            assert_eq!(docs, ui.page_count() - 1, "it is the last page");
            let rows: Vec<&Row> = ui.pages[docs].cards.iter().flat_map(|c| &c.rows).collect();
            assert!(rows.len() > 60 && rows.iter().all(|r| matches!(r.ctl, Ctl::None)), "every row is words only");
            let mut ids: Vec<&str> = rows.iter().map(|r| r.id).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), rows.len(), "every row has its own id");
            assert!(ui.values.keys().all(|k| !k.starts_with("doc/")), "no value is read or saved for them");
            for t in ["commands", "guide", "recipes", "options", "files"] {
                assert!(ui.pages[docs].cards.iter().any(|c| c.title.starts_with(t)), "{t}");
            }
            // searching "tile" finds a setting and the documentation about tiling
            ui.type_text("tile");
            let found = view(&ui.pages, ui.page, &ui.query);
            assert!(found.iter().any(|(_, r)| r.iter().any(|r| matches!(r.ctl, Ctl::None))), "docs rows are found");
            // and clicking on words does nothing, anywhere on the page
            ui.escape();
            ui.go(docs);
            let (w, h) = SCREEN;
            for (x, y) in [(300.0, 300.0), (700.0, 400.0), (1200.0, 500.0), (900.0, 700.0)] {
                assert!(matches!(ui.press(x, y, w, h, false), Effect::None));
            }
            assert!(!config::path("look").exists(), "nothing was written");
        });
    }

    #[test]
    fn controls_sit_beside_their_words_except_sliders_and_wide_choices() {
        with_config(|| {
            let mut ui = Ui::new();
            ui.fit(1920.0, 1080.0);
            let lay = ui.lay();
            let row = |id: &str| lay.rows.iter().find(|p| p.id == id).unwrap();
            let toggle = row("motion/reduce");
            let (x, _, w, _) = toggle.row;
            assert!((toggle.ctl_box.0 + toggle.ctl_box.2 - (x + w - INNER)).abs() < 0.01, "a toggle is at the card's right edge");
            let slider = row("sky/nebula");
            assert!(slider.ctl_box.1 > slider.row.1 + 50.0, "a slider is under its words");
            assert!(row("look/style").ctl_box.1 > row("look/style").row.1 + 50.0, "four segments do not fit beside the words");
            assert!(lay.cards.len() == 3 && lay.total > 0.0);
        });
    }
}
