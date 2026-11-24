//! The wonder window: a card like help and settings, with two pages (now, find) on the star thread, the
//! search pill on top and the queue as a thread of stars along the bottom.
use crate::cover;
use crate::lyrics::{self, Lyrics};
use crate::player::{Shared, State};
use crate::search::{self, Track};
use seiza_common::draw::{blend, shared_font, Canvas, Text};
use seiza_common::kit::{self, Glide, Thread, PILL_W};
use seiza_common::palette::{palette, rgba};
use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

const PAD: f32 = 30.0; // between the card's edge and its content
const FOOT: f32 = 60.0;
const LEFT_W: f32 = 360.0; // the cover and what sits under it
const GAP: f32 = 40.0; // between that column and the lyrics
const ROW_H: f32 = 76.0; // a search result
const ROW_GAP: f32 = 10.0;
const QUEUE_MAX: usize = 7; // stars shown at once
const PAGES: [&str; 2] = ["now", "find"];

/// The mini pill: its size, and where the cover (x, y, radius), the seek thread and the play button sit in it.
pub const MINI: (u32, u32) = (480, 76);
const MINI_COVER: (f32, f32, f32) = (39.0, 38.0, 22.0);
const MINI_TEXT_X: f32 = 75.0;
const MINI_SEEK: (f32, f32, f32) = (35.0, 445.0, 62.0);
const MINI_PLAY: (f32, f32, f32) = (441.0, 36.0, 17.0);

/// What a click on the pill asks for.
pub enum Mini {
    Stay,
    Expand,
}

/// A job running on another thread.
enum Job<T> {
    Idle,
    Busy,
    Done(T),
    Failed(String),
}

enum Msg {
    Results(String, Result<Vec<Track>, String>),
    Lyrics(String, Option<Lyrics>),
    Cover(String, Option<Canvas>),
}

pub struct Ui {
    shared: Option<Shared>, // None for --dump
    pub page: usize,
    query: String,
    searched: String, // the words the results are for
    results: Job<Vec<Track>>,
    sel: usize,
    added: HashSet<String>, // ids put in the queue from the results
    queue: Vec<Track>,
    at: Option<usize>,
    st: State,
    shuffle: bool,
    repeat: bool,
    track_id: Option<String>, // the track the lyrics and cover below are for
    lyrics: Job<Lyrics>,
    cover: Option<Canvas>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    w: f32,
    h: f32,
    hover: Option<usize>,
    glide: Glide,
    stale: bool,
    shown: String, // what was last drawn, to tell whether a tick changed anything
    content: Option<Canvas>,
    back: Option<Canvas>,
    t0: Instant,
    t13: Text,
    t15: Text,
    t18: Text,
    t22: Text,
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// `s` cut to `room` with ".." at the end when it does not fit.
fn fit(t: &mut Text, s: &str, room: f32) -> String {
    if t.width(s) <= room {
        return s.to_string();
    }
    let mut out = s.to_string();
    while !out.is_empty() && t.width(&format!("{out}..")) > room {
        out.pop();
    }
    format!("{out}..")
}

/// Where everything sits, for drawing and for hit tests alike.
struct Geo {
    ox: f32,
    oy: f32,
    cover: (f32, f32),
    seek: (f32, f32, f32), // x0, x1, y
    buttons: [(f32, f32, f32); 5], // prev, play, next, shuffle, repeat: centre and radius
    lyrics: (f32, f32, f32, f32), // x, top, width, bottom
    lyric_mid: f32, // the baseline of the line being sung
    queue_y: f32,
    queue: Thread,
    tabs: Thread,
}

impl Ui {
    pub fn new(shared: Option<Shared>) -> Ui {
        let (tx, rx) = channel();
        let f = shared_font();
        Ui {
            shared,
            page: 0,
            query: String::new(),
            searched: String::new(),
            results: Job::Idle,
            sel: 0,
            added: HashSet::new(),
            queue: vec![],
            at: None,
            st: State { idle: true, ..Default::default() },
            shuffle: false,
            repeat: false,
            track_id: None,
            lyrics: Job::Idle,
            cover: None,
            tx,
            rx,
            w: 1200.0,
            h: 740.0,
            hover: None,
            glide: Glide::default(),
            stale: true,
            shown: String::new(),
            content: None,
            back: None,
            t0: Instant::now(),
            t13: Text::new(f.clone(), 13.0),
            t15: Text::new(f.clone(), 15.0),
            t18: Text::new(f.clone(), 18.0),
            t22: Text::new(f, 22.0),
        }
    }

    /// A queue and a song in the middle of a verse, for `--dump`.
    pub fn demo(page: usize) -> Ui {
        let mut ui = Ui::new(None);
        let t = |id: &str, title: &str, artist: &str, secs| Track { id: id.into(), title: title.into(), artist: artist.into(), secs };
        ui.queue = vec![
            t("a", "sleep rewind", "kotoza demo", 180),
            t("b", "paper boats", "tiny orchestra", 200),
            t("c", "hoshi no ito", "kotoza demo", 222),
            t("d", "night walk, slowly", "tiny orchestra", 240),
            t("e", "lullaby for a fallen angel", "someone", 210),
            t("f", "the sky is not over", "someone else", 230),
            t("g", "dawn, a little later", "kotoza demo", 200),
        ];
        ui.at = Some(2);
        ui.st = State { pos: 100.0, len: 222.0, ..Default::default() };
        ui.track_id = Some("c".into());
        let line = |t: f32, s: &str| (t, s.to_string());
        ui.lyrics = Job::Done(Lyrics {
            synced: true,
            lines: vec![line(60.0, "指でなぞる 細い糸の先"), line(80.0, "君がいた気がした"), line(98.0, "眠れないなら 一緒に歩こう"), line(120.0, "空はまだ 終わらないから"), line(140.0, "手を伸ばして 光に触れて")],
        });
        ui.page = page;
        ui.query = if page == 1 { "hoshi".into() } else { String::new() };
        ui.searched = ui.query.clone();
        ui.results = Job::Done(ui.queue[..5].to_vec());
        ui
    }

    fn fit(&mut self, w: f32, h: f32) {
        self.w = (w * 0.83).clamp(1000.0, 1700.0).min(w - 40.0);
        self.h = (h * 0.84).clamp(740.0, 940.0).min(h - 40.0);
    }

    fn geo(&self, w: f32, h: f32) -> Geo {
        let ox = ((w - self.w) / 2.0).round();
        let oy = ((h - self.h) / 2.0 + 18.0).round();
        let lx = ox + PAD;
        let (cy, qy) = (oy + 148.0, oy + self.h - 105.0);
        let n = self.queue.len().clamp(1, QUEUE_MAX);
        Geo {
            ox,
            oy,
            cover: (lx, cy),
            seek: (lx, lx + LEFT_W, oy + 441.0),
            buttons: [(lx + LEFT_W / 2.0 - 70.0, oy + 508.0, 19.0), (lx + LEFT_W / 2.0, oy + 508.0, 26.0), (lx + LEFT_W / 2.0 + 70.0, oy + 508.0, 19.0), (lx + LEFT_W / 2.0 - 125.0, oy + 508.0, 14.0), (lx + LEFT_W / 2.0 + 125.0, oy + 508.0, 14.0)],
            lyrics: (lx + LEFT_W + GAP, oy + 175.0, self.w - 2.0 * PAD - LEFT_W - GAP, oy + 475.0),
            lyric_mid: oy + 330.0,
            queue_y: qy,
            queue: Thread::new(lx, ox + self.w - PAD, qy, n),
            tabs: Thread::new(ox + 150.0, ox + 150.0 + 150.0 * PAGES.len() as f32, oy + 40.0, PAGES.len()),
        }
    }

    /// The part of the queue on show, and where it starts: the current track stays in view.
    fn queue_window(&self) -> (usize, &[Track]) {
        let start = self.at.map_or(0, |a| a.saturating_sub(QUEUE_MAX / 2)).min(self.queue.len().saturating_sub(QUEUE_MAX));
        (start, &self.queue[start..(start + QUEUE_MAX).min(self.queue.len())])
    }

    fn current(&self) -> Option<&Track> {
        self.at.and_then(|i| self.queue.get(i))
    }

    /// True while mpv is fetching the track: a length is the first thing it learns.
    fn loading(&self) -> bool {
        self.at.is_some() && self.st.len == 0.0
    }

    fn with_player(&self, f: impl FnOnce(&mut crate::player::Player)) {
        if let Some(s) = &self.shared {
            f(&mut s.lock().unwrap());
        }
    }

    // ---- what comes in ----

    /// About four times a second: take what the threads found and the engine's state. True if a frame is due.
    pub fn tick(&mut self) -> bool {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Results(q, r) if q == self.searched => {
                    self.sel = 0;
                    self.results = match r {
                        Ok(v) if v.is_empty() => Job::Failed("nothing found.. try other words".into()),
                        Ok(v) => Job::Done(v),
                        Err(e) => Job::Failed(e),
                    }
                }
                Msg::Lyrics(id, l) if self.track_id.as_deref() == Some(&id) => self.lyrics = l.map_or(Job::Failed(String::new()), Job::Done),
                Msg::Cover(id, c) if self.track_id.as_deref() == Some(&id) => self.cover = c,
                _ => {}
            }
        }
        if let Some(s) = &self.shared {
            let p = s.lock().unwrap();
            (self.queue, self.at, self.st) = (p.queue.items.clone(), p.queue.at, p.last.clone());
            (self.shuffle, self.repeat) = (p.shuffle, p.repeat);
        }
        let id = self.current().map(|t| t.id.clone());
        if id != self.track_id {
            self.track_id = id;
            (self.cover, self.lyrics) = (None, Job::Idle);
            if let Some(t) = self.current().cloned() {
                self.lyrics = Job::Busy;
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Msg::Lyrics(t.id.clone(), lyrics::fetch(&t)));
                    let _ = tx.send(Msg::Cover(t.id.clone(), cover::fetch(&t)));
                });
            }
        }
        let key = self.state_key();
        let changed = key != self.shown;
        self.stale |= changed;
        changed
    }

    /// Everything a tick can change that shows.
    fn state_key(&self) -> String {
        let line = match &self.lyrics {
            Job::Done(l) => l.current(self.st.pos as f32),
            _ => None,
        };
        let r = match &self.results {
            Job::Idle => 0,
            Job::Busy => 1,
            Job::Done(v) => 2 + v.len(),
            Job::Failed(_) => 99,
        };
        format!("{:?}|{}|{}|{}|{}|{:?}|{}|{}|{}", self.at, self.queue.len(), self.st.paused, self.st.pos as u32, self.st.len as u32, line, self.cover.is_some(), r, self.track_id.is_some())
    }

    /// True while a frame is still owed: something changed, or something moves by itself.
    pub fn pending(&self) -> bool {
        self.stale || self.animating()
    }

    fn animating(&self) -> bool {
        self.glide.active() || self.loading() || matches!(self.results, Job::Busy) && self.page == 1
    }

    // ---- keys ----

    pub fn type_str(&mut self, s: &str) {
        self.query.push_str(s);
        self.stale = true;
    }

    /// Ctrl+V: the clipboard goes into the pill, and a link is looked up at once.
    pub fn paste(&mut self) {
        let Ok(out) = std::process::Command::new("wl-paste").args(["--no-newline", "--type", "text"]).output() else { return };
        let text = String::from_utf8_lossy(&out.stdout).trim().replace('\n', " ");
        self.type_str(&text);
        if text.starts_with("http") {
            self.enter();
        }
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.stale = true;
    }

    /// Esc: empty the search pill first. False when it was already empty.
    pub fn clear_query(&mut self) -> bool {
        let had = !self.query.is_empty();
        self.query.clear();
        self.stale |= had;
        had
    }

    pub fn searching(&self) -> bool {
        !self.query.is_empty()
    }

    pub fn go(&mut self, page: usize) {
        if page < PAGES.len() && page != self.page {
            self.glide.start(self.page as f32);
            self.page = page;
            self.stale = true;
        }
    }

    /// Enter: a new search when the words changed, else the chosen result joins the queue.
    pub fn enter(&mut self) {
        let q = self.query.trim().to_string();
        if !q.is_empty() && q != self.searched {
            self.go(1);
            self.searched = q.clone();
            self.results = Job::Busy;
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let r = search::search(&q, 5);
                let _ = tx.send(Msg::Results(q, r));
            });
        } else if self.page == 1
            && let Some(t) = self.selected()
        {
            self.added.insert(t.id.clone());
            self.with_player(|p| p.add(t));
        }
        self.stale = true;
    }

    fn selected(&self) -> Option<Track> {
        match &self.results {
            Job::Done(v) => v.get(self.sel).cloned(),
            _ => None,
        }
    }

    pub fn move_sel(&mut self, d: isize) {
        if let Job::Done(v) = &self.results {
            self.sel = (self.sel as isize + d).clamp(0, v.len() as isize - 1) as usize;
            self.stale = true;
        }
    }

    pub fn toggle(&mut self) {
        let paused = self.st.paused;
        self.st.paused = !paused;
        self.with_player(|p| { p.pause(!paused); });
        self.stale = true;
    }

    pub fn step(&mut self, d: isize) {
        self.with_player(|p| if d > 0 { p.next() } else { p.prev() });
    }

    // ---- the pointer ----

    pub fn hover(&mut self, at: Option<(f32, f32)>, w: f32, h: f32) -> bool {
        self.fit(w, h);
        let now = at.and_then(|(x, y)| self.geo(w, h).tabs.hit(x, y));
        let changed = std::mem::replace(&mut self.hover, now) != now;
        self.stale |= changed;
        changed
    }

    /// A click. True if something changed and a frame is due.
    pub fn press(&mut self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.fit(w, h);
        let g = self.geo(w, h);
        if let Some(i) = g.tabs.hit(x, y) {
            self.go(i);
            return true;
        }
        self.stale = true;
        if self.page == 1 {
            let top = g.oy + 168.0;
            let row = ((y - top) / (ROW_H + ROW_GAP)).floor();
            if x > g.ox + PAD && x < g.ox + self.w - PAD && row >= 0.0 && (y - top) % (ROW_H + ROW_GAP) < ROW_H {
                self.sel = row as usize;
                if let Some(t) = self.selected() {
                    self.with_player(|p| p.play_now(t));
                    self.go(0);
                }
            }
            return true;
        }
        let (sx0, sx1, sy) = g.seek;
        if (y - sy).abs() < 14.0 && x >= sx0 - 4.0 && x <= sx1 + 4.0 && self.st.len > 0.0 {
            let at = ((x - sx0) / (sx1 - sx0)).clamp(0.0, 1.0) as f64 * self.st.len;
            self.with_player(|p| { p.seek(at); });
            return true;
        }
        for (i, (bx, by, r)) in g.buttons.iter().enumerate() {
            if (x - bx).hypot(y - by) <= r + 4.0 {
                match i {
                    0 => self.step(-1),
                    1 => self.toggle(),
                    2 => self.step(1),
                    3 => {
                        self.shuffle = !self.shuffle;
                        let on = self.shuffle;
                        self.with_player(|p| p.shuffle = on);
                    }
                    _ => {
                        self.repeat = !self.repeat;
                        let on = self.repeat;
                        self.with_player(|p| p.repeat = on);
                    }
                }
                return true;
            }
        }
        if let Some(i) = g.queue.hit(x, y).filter(|_| !self.queue.is_empty()) {
            let at = self.queue_window().0 + i;
            self.with_player(|p| { p.play_at(at); });
            return true;
        }
        if let Some(i) = self.lyric_at(&g, x, y)
            && let Job::Done(l) = &self.lyrics
        {
            let t = l.lines[i].0 as f64;
            self.with_player(|p| { p.seek(t); });
        }
        true
    }

    /// The lyric line drawn at (x, y), if the lyrics are timed.
    fn lyric_at(&self, g: &Geo, x: f32, y: f32) -> Option<usize> {
        let Job::Done(l) = &self.lyrics else { return None };
        let cur = l.current(self.st.pos as f32)?;
        let (lx, top, w, bottom) = g.lyrics;
        if x < lx || x > lx + w || y < top || y > bottom {
            return None;
        }
        (0..l.lines.len()).find(|&i| (y - (g.lyric_mid + line_offset(cur, i))).abs() < 22.0 && !l.lines[i].1.is_empty())
    }

    // ---- drawing ----

    pub fn render(&mut self, w: usize, h: usize, e: f32) -> Canvas {
        self.fit(w as f32, h as f32);
        if self.stale || self.animating() || self.content.as_ref().is_none_or(|c| c.w != w || c.h != h) {
            self.shown = self.state_key();
            self.content = Some(self.draw_card(w, h));
            self.stale = false;
            if !self.glide.active() {
                self.glide.clear();
            }
        }
        let oy = self.geo(w as f32, h as f32).oy;
        kit::appear(self.content.as_ref().expect("drawn above"), e, (oy as usize).saturating_sub(24)..(oy + self.h) as usize + 24)
    }

    fn draw_card(&mut self, w: usize, h: usize) -> Canvas {
        let p = palette();
        let g = self.geo(w as f32, h as f32);
        if self.back.as_ref().is_none_or(|b| b.w != w || b.h != h) {
            let mut b = kit::window_plate(w, h, (g.ox, g.oy), (self.w, self.h), FOOT);
            self.t22.draw(&mut b, g.ox + PAD, g.oy + 46.0, "wonder", rgba(p.text, 0.95));
            self.back = Some(b);
        }
        let mut cv = self.back.clone().expect("made above");
        let labels: Vec<&str> = PAGES.to_vec();
        g.tabs.draw(&mut cv, &mut self.t13, &labels, self.glide.pos(self.page), self.hover);
        kit::search_pill(&mut cv, &mut self.t15, g.ox + self.w - PAD - PILL_W, g.oy + 23.0, &self.query, "search the sky..");
        if self.page == 0 {
            self.draw_now(&mut cv, &g);
        } else {
            self.draw_find(&mut cv, &g);
        }
        let base = g.oy + self.h - 22.0;
        let hint = if self.page == 0 { "click a star to change page  \u{b7}  type or paste to search  \u{b7}  space: play or pause  \u{b7}  left/right: previous, next" } else { "enter: search, then add to the queue  \u{b7}  up/down: choose  \u{b7}  click: play now" };
        self.t13.draw(&mut cv, g.ox + PAD, base, hint, rgba(p.text, 0.4));
        let quit = "ctrl+m: mini  \u{b7}  Esc: clear / close";
        let qw = self.t13.width(quit);
        self.t13.draw(&mut cv, g.ox + self.w - PAD - qw, base, quit, rgba(p.text, 0.4));
        cv
    }

    fn heading(&mut self, cv: &mut Canvas, g: &Geo, title: &str, blurb: &str) {
        let p = palette();
        self.t22.draw(cv, g.ox + PAD, g.oy + 108.0, title, rgba(p.text, 1.0));
        self.t13.draw(cv, g.ox + PAD, g.oy + 130.0, blurb, rgba(p.text, 0.55));
    }

    fn draw_now(&mut self, cv: &mut Canvas, g: &Geo) {
        let p = palette();
        let blurb = if self.current().is_some() { "from youtube" } else { "nothing yet.. search the sky and press enter" };
        self.heading(cv, g, "now playing", blurb);
        let (cx, cy) = g.cover;
        // the cover's frame, with a glow and a star when there is no picture
        cv.round_rect(cx - 1.0, cy - 1.0, cover::W as f32 + 2.0, cover::H as f32 + 2.0, 15.0, rgba(p.text, 0.12));
        match &self.cover {
            Some(c) => cv.over(c, cx as i32, cy as i32),
            None => {
                cv.round_rect(cx, cy, cover::W as f32, cover::H as f32, 14.0, rgba(p.sky, 1.0));
                let (mx, my) = (cx + cover::W as f32 / 2.0, cy + cover::H as f32 / 2.0);
                for k in (1..=4).rev() {
                    let r = 24.0 + k as f32 * 22.0;
                    cv.round_rect(mx - r, my - r, 2.0 * r, 2.0 * r, r, rgba(p.glow, 0.035));
                }
                cv.sparkle(mx, my, 26.0, rgba(p.star, 0.95));
            }
        }
        let Some(t) = self.current().cloned() else { return self.draw_queue(cv, g) };
        let loading = self.loading();
        let title = fit(&mut self.t22, if loading { "fetching.." } else { &t.title }, LEFT_W);
        self.t22.draw(cv, cx, g.oy + 387.0, &title, rgba(p.text, if loading { 0.6 } else { 1.0 }));
        let by = if t.artist.is_empty() { "youtube".to_string() } else { format!("{} \u{b7} youtube", t.artist) };
        let by = fit(&mut self.t13, &if loading { t.title.clone() } else { by }, LEFT_W);
        self.t13.draw(cv, cx, g.oy + 409.0, &by, rgba(p.text, 0.55));

        self.seek_thread(cv, g.seek);
        if !loading {
            let (x0, x1, y) = g.seek;
            self.t13.draw(cv, x0, y + 25.0, &clock(self.st.pos), rgba(p.text, 0.45));
            let total = clock(self.st.len);
            let tw = self.t13.width(&total);
            self.t13.draw(cv, x1 - tw, y + 25.0, &total, rgba(p.text, 0.45));
        }
        self.draw_buttons(cv, g);
        self.draw_lyrics(cv, g);
        self.draw_queue(cv, g);
    }

    /// The seek thread: a gold wave, brighter where it has been, with a sparkle where it is; a ripple while fetching.
    fn seek_thread(&self, cv: &mut Canvas, (x0, x1, y): (f32, f32, f32)) {
        let p = palette();
        if self.loading() {
            let ph = self.t0.elapsed().as_secs_f32() * 6.0;
            let pts: Vec<(f32, f32)> = (0..=60).map(|i| (x0 + (x1 - x0) * i as f32 / 60.0, y + 5.0 * (i as f32 * 0.5 - ph).sin())).collect();
            cv.polyline(&pts, 1.6, rgba(p.star, 0.8));
            return;
        }
        let wob = |x: f32| y + 2.2 * (x * 0.045).sin() + 1.2 * (x * 0.11 + 1.0).sin();
        let f = if self.st.len > 0.0 { (self.st.pos / self.st.len).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let xm = x0 + (x1 - x0) * f;
        let pts = |a: f32, b: f32| -> Vec<(f32, f32)> { (0..=((b - a) / 4.0).ceil().max(1.0) as usize).map(|i| { let x = (a + i as f32 * 4.0).min(b); (x, wob(x)) }).collect() };
        cv.polyline(&pts(xm, x1), 1.3, rgba(p.star, 0.3));
        cv.polyline(&pts(x0, xm), 1.8, rgba(p.star, 0.95));
        cv.round_rect(xm - 14.0, wob(xm) - 14.0, 28.0, 28.0, 14.0, rgba(p.star, 0.08));
        cv.sparkle(xm, wob(xm), 9.0, rgba(p.star, 1.0));
    }

    // ---- the mini pill ----

    /// What the pill says: the line being sung, or else the track; and the small line under it.
    fn mini_lines(&self) -> (String, String) {
        let Some(t) = self.current() else { return ("nothing playing".into(), "open wonder and search the sky..".into()) };
        if self.loading() {
            return ("fetching..".into(), t.title.clone());
        }
        let line = match &self.lyrics {
            Job::Done(l) if l.synced => l.current(self.st.pos as f32).map(|i| l.lines[i].1.clone()).filter(|s| !s.is_empty()),
            _ => None,
        };
        let by = |s: &str| if t.artist.is_empty() { s.to_string() } else { format!("{} \u{b7} {s}", t.artist) };
        match (line, &self.lyrics) {
            (Some(line), _) => (line, by(&t.title)),
            (None, Job::Busy) => (t.title.clone(), t.artist.clone()),
            (None, _) => (t.title.clone(), by("no lyrics for this one..")),
        }
    }

    pub fn render_mini(&mut self) -> Canvas {
        let p = palette();
        let (w, h) = (MINI.0 as usize, MINI.1 as usize);
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        cv.round_rect(0.0, 0.0, w as f32, h as f32, h as f32 / 2.0, rgba(p.star, 0.3));
        cv.round_rect(1.2, 1.2, w as f32 - 2.4, h as f32 - 2.4, h as f32 / 2.0 - 1.2, rgba(p.panel, 0.96));

        // the cover as a disc, or a dim sky with a star when there is no picture
        let (cx, cy, r) = MINI_COVER;
        match &self.cover {
            Some(c) => {
                let d = (2.0 * r) as usize;
                for y in 0..d {
                    for x in 0..d {
                        let (dx, dy) = (x as f32 + 0.5 - r, y as f32 + 0.5 - r);
                        let cover = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
                        let s = c.px[(y * c.h / d) * c.w + (c.w - c.h) / 2 + x * c.h / d]; // the middle square of the picture
                        let rgb = [(s >> 16 & 255) as f32 / 255.0, (s >> 8 & 255) as f32 / 255.0, (s & 255) as f32 / 255.0, 1.0];
                        blend(&mut cv.px[(y + (cy - r) as usize) * w + x + (cx - r) as usize], rgb, cover);
                    }
                }
            }
            None => {
                cv.round_rect(cx - r, cy - r, 2.0 * r, 2.0 * r, r, rgba(p.sky, 1.0));
                cv.sparkle(cx, cy, 8.0, rgba(p.star, 0.8));
            }
        }

        let room = MINI_PLAY.0 - MINI_PLAY.2 - 14.0 - MINI_TEXT_X;
        let (title, sub) = self.mini_lines();
        let (title, sub) = (fit(&mut self.t15, &title, room), fit(&mut self.t13, &sub, room));
        self.t15.draw(&mut cv, MINI_TEXT_X, 33.0, &title, rgba(p.text, 1.0));
        self.t13.draw(&mut cv, MINI_TEXT_X, 51.0, &sub, rgba(p.text, 0.55));
        self.seek_thread(&mut cv, MINI_SEEK);

        let (x, y, r) = MINI_PLAY;
        let ink = rgba(p.star, 1.0);
        cv.round_rect(x - r, y - r, 2.0 * r, 2.0 * r, r, rgba(p.star, if self.loading() { 0.35 } else { 0.9 }));
        cv.round_rect(x - r + 1.2, y - r + 1.2, 2.0 * r - 2.4, 2.0 * r - 2.4, r - 1.2, rgba(p.panel, 1.0));
        if self.st.paused || self.st.idle {
            cv.shape(x + 1.0, y, 8.0, ink, |px, py| px - x >= -5.0 && px - x <= 6.0 && (py - y).abs() <= (6.0 - (px - x)) * 0.57);
        } else {
            cv.round_rect(x - 5.5, y - 6.0, 4.0, 12.0, 1.2, ink);
            cv.round_rect(x + 1.5, y - 6.0, 4.0, 12.0, 1.2, ink);
        }
        self.shown = self.state_key();
        self.stale = false;
        cv
    }

    pub fn press_mini(&mut self, x: f32, y: f32) -> Mini {
        let (px, py, pr) = MINI_PLAY;
        let (cx, cy, cr) = MINI_COVER;
        let (sx0, sx1, sy) = MINI_SEEK;
        if (x - px).hypot(y - py) <= pr + 4.0 {
            self.toggle();
        } else if (x - cx).hypot(y - cy) <= cr + 4.0 {
            return Mini::Expand;
        } else if (y - sy).abs() < 8.0 && x >= sx0 - 4.0 && x <= sx1 + 4.0 && self.st.len > 0.0 {
            let at = ((x - sx0) / (sx1 - sx0)).clamp(0.0, 1.0) as f64 * self.st.len;
            self.with_player(|p| { p.seek(at); });
        }
        Mini::Stay
    }

    fn draw_buttons(&mut self, cv: &mut Canvas, g: &Geo) {
        let p = palette();
        let ink = rgba(p.text, 0.85);
        for (i, &(x, y, r)) in g.buttons.iter().enumerate() {
            let on = match i {
                3 => self.shuffle,
                4 => self.repeat,
                _ => i == 1,
            };
            let edge = if on { rgba(p.star, 0.9) } else { rgba(p.text, 0.28) };
            cv.round_rect(x - r, y - r, 2.0 * r, 2.0 * r, r, edge);
            cv.round_rect(x - r + 1.2, y - r + 1.2, 2.0 * r - 2.4, 2.0 * r - 2.4, r - 1.2, rgba(p.panel, 1.0));
            match i {
                0 => {
                    cv.round_rect(x - 7.0, y - 6.5, 2.0, 13.0, 1.0, ink);
                    cv.shape(x + 2.0, y, 8.0, ink, |px, py| px - x >= -4.0 && px - x <= 7.0 && (py - y).abs() <= (px - x + 4.0) * 0.6);
                }
                3 | 4 => {
                    let (word, ink) = (["s", "r"][i - 3], if on { rgba(p.star, 1.0) } else { rgba(p.text, 0.5) });
                    let w = self.t13.width(word);
                    self.t13.draw(cv, x - w / 2.0, y + 4.5, word, ink);
                }
                2 => {
                    cv.round_rect(x + 5.0, y - 6.5, 2.0, 13.0, 1.0, ink);
                    cv.shape(x - 2.0, y, 8.0, ink, |px, py| px - x >= -7.0 && px - x <= 4.0 && (py - y).abs() <= (4.0 - (px - x)) * 0.6);
                }
                _ if !self.st.paused => {
                    cv.round_rect(x - 7.0, y - 8.0, 5.0, 16.0, 1.5, rgba(p.star, 1.0));
                    cv.round_rect(x + 2.0, y - 8.0, 5.0, 16.0, 1.5, rgba(p.star, 1.0));
                }
                _ => cv.shape(x + 1.0, y, 10.0, rgba(p.star, 1.0), |px, py| px - x >= -6.0 && px - x <= 8.0 && (py - y).abs() <= (8.0 - (px - x)) * 0.57),
            }
        }
    }

    fn draw_lyrics(&mut self, cv: &mut Canvas, g: &Geo) {
        let p = palette();
        let (lx, top, w, _) = g.lyrics;
        let (head, body) = match &self.lyrics {
            Job::Done(l) if l.synced => ("lyrics \u{b7} synced, click a line to jump", None),
            Job::Done(_) => ("lyrics \u{b7} not synced, so no line is followed", None),
            Job::Busy => ("lyrics", Some("looking for lyrics..")),
            _ => ("lyrics", Some("no lyrics for this one..")),
        };
        self.t13.draw(cv, lx, g.oy + 160.0, head, rgba(p.text, 0.5));
        if let Some(text) = body {
            self.t18.draw(cv, lx, g.lyric_mid, text, rgba(p.text, 0.4));
            return;
        }
        let Job::Done(l) = self.lyrics.clone_done() else { return };
        match l.current(self.st.pos as f32) {
            Some(cur) => {
                for i in cur.saturating_sub(4)..(cur + 5).min(l.lines.len()) {
                    let (d, text) = ((i as isize - cur as isize).unsigned_abs(), &l.lines[i].1);
                    let base = g.lyric_mid + line_offset(cur, i);
                    if text.is_empty() || base < top || d > 3 {
                        continue;
                    }
                    let a = [1.0, 0.5, 0.28, 0.14][d];
                    let t = if d == 0 { &mut self.t22 } else { &mut self.t18 };
                    let shown = fit(t, text, w);
                    t.draw(cv, lx, base, &shown, rgba(p.text, a));
                }
            }
            None => {
                // not synced, or the song has not reached its first line: the first lines, quietly
                for (k, (_, text)) in l.lines.iter().filter(|(_, s)| !s.is_empty()).take(8).enumerate() {
                    let shown = fit(&mut self.t18, text, w);
                    self.t18.draw(cv, lx, top + 28.0 + k as f32 * 36.0, &shown, rgba(p.text, 0.55));
                }
            }
        }
    }

    fn draw_queue(&mut self, cv: &mut Canvas, g: &Geo) {
        let p = palette();
        let lx = g.ox + PAD;
        if self.queue.is_empty() {
            self.t13.draw(cv, lx, g.queue_y - 10.0, "queue \u{b7} empty", rgba(p.text, 0.5));
            return;
        }
        let n = self.queue.len();
        self.t13.draw(cv, lx, g.queue_y - 50.0, &format!("queue \u{b7} a constellation of {n}"), rgba(p.text, 0.5));
        let start = self.queue_window().0;
        let titles: Vec<String> = self.queue_window().1.iter().map(|t| t.title.clone()).collect();
        let names: Vec<String> = titles.iter().map(|t| fit(&mut self.t13, t, 150.0)).collect();
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        let pos = self.at.map_or(-9.0, |a| (a - start) as f32);
        g.queue.draw(cv, &mut self.t13, &labels, pos, None);
    }

    fn draw_find(&mut self, cv: &mut Canvas, g: &Geo) {
        let p = palette();
        let (title, blurb) = match &self.results {
            Job::Idle => ("find".to_string(), "type words (or a link) in the pill above and press enter".to_string()),
            Job::Busy => ("find".to_string(), format!("looking for \"{}\"..", self.searched)),
            Job::Done(v) => ("find".to_string(), format!("{} results for \"{}\"", v.len(), self.searched)),
            Job::Failed(_) => ("find".to_string(), format!("\"{}\"", self.searched)),
        };
        self.heading(cv, g, &title, &blurb);
        let top = g.oy + 168.0;
        let (x, rw) = (g.ox + PAD, self.w - 2.0 * PAD);
        match &self.results {
            Job::Busy => {
                let ph = self.t0.elapsed().as_secs_f32() * 6.0;
                let pts: Vec<(f32, f32)> = (0..=80).map(|i| (x + (rw - 40.0) * i as f32 / 80.0, top + 40.0 + 6.0 * (i as f32 * 0.4 - ph).sin())).collect();
                cv.polyline(&pts, 1.6, rgba(p.star, 0.8));
                self.t13.draw(cv, x, top + 80.0, "fetching..", rgba(p.text, 0.5));
            }
            Job::Failed(e) => {
                self.t18.draw(cv, x, top + 40.0, &format!("oops.. {e}"), rgba(p.text, 0.6));
            }
            Job::Done(v) => {
                for (i, t) in v.iter().enumerate() {
                    let y = top + i as f32 * (ROW_H + ROW_GAP);
                    let hot = i == self.sel;
                    cv.round_rect(x, y, rw, ROW_H, 14.0, rgba(if hot { p.star } else { p.text }, if hot { 0.8 } else { 0.10 }));
                    cv.round_rect(x + 1.0, y + 1.0, rw - 2.0, ROW_H - 2.0, 13.0, rgba(p.panel, 1.0));
                    cv.round_rect(x + 12.0, y + 11.0, 96.0, 54.0, 8.0, rgba(p.sky, 1.0));
                    cv.sparkle(x + 60.0, y + 38.0, 9.0, rgba(p.star, 0.8));
                    let room = rw - 140.0 - 110.0;
                    let name = fit(&mut self.t15, &t.title, room);
                    self.t15.draw(cv, x + 124.0, y + 32.0, &name, rgba(p.text, 1.0));
                    let by = if t.artist.is_empty() { "unknown channel".to_string() } else { t.artist.clone() };
                    let len = if t.secs > 0 { clock(t.secs as f64) } else { "live".into() };
                    self.t13.draw(cv, x + 124.0, y + 54.0, &format!("{by} \u{b7} {len}"), rgba(p.text, 0.55));
                    if self.added.contains(&t.id) {
                        let s = "in the queue";
                        let sw = self.t13.width(s);
                        self.t13.draw(cv, x + rw - 18.0 - sw, y + 43.0, s, rgba(p.star, 0.9));
                    }
                }
            }
            Job::Idle => {}
        }
    }
}

/// How far below the line being sung line `i` is drawn (above when negative): the one being sung gets room.
fn line_offset(cur: usize, i: usize) -> f32 {
    let d = i as f32 - cur as f32;
    d * 46.0 + d.signum() * 10.0
}

impl Job<Lyrics> {
    fn clone_done(&self) -> Job<Lyrics> {
        match self {
            Job::Done(l) => Job::Done(l.clone()),
            _ => Job::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_then_enter_goes_to_find_and_esc_clears_before_it_closes() {
        let mut ui = Ui::new(None);
        ui.type_str("hoshi");
        ui.backspace();
        assert!(ui.searching());
        ui.type_str("i");
        ui.enter();
        assert_eq!((ui.page, ui.searched.as_str()), (1, "hoshi"));
        assert!(matches!(ui.results, Job::Busy));
        assert!(ui.clear_query());
        assert!(!ui.clear_query(), "already empty: Esc closes");
    }

    #[test]
    fn a_click_on_a_page_star_changes_page() {
        let (w, h) = (1920.0, 1080.0);
        let mut ui = Ui::new(None);
        ui.fit(w, h);
        let g = ui.geo(w, h);
        let (x, y) = g.tabs.star(1);
        assert!(ui.press(x, y + 10.0, w, h));
        assert_eq!(ui.page, 1);
    }

    #[test]
    fn the_lyric_being_sung_is_found_by_its_place_on_screen() {
        let (w, h) = (1920.0, 1080.0);
        let mut ui = Ui::demo(0);
        ui.fit(w, h);
        let g = ui.geo(w, h);
        let (lx, ..) = g.lyrics;
        assert_eq!(ui.lyric_at(&g, lx + 20.0, g.lyric_mid - 5.0), Some(2), "the middle line is the one at 1:40");
        assert_eq!(ui.lyric_at(&g, lx + 20.0, g.lyric_mid + line_offset(2, 3)), Some(3));
        assert_eq!(ui.lyric_at(&g, lx - 20.0, g.lyric_mid), None, "left of the lyrics");
    }

    #[test]
    fn the_pill_shows_the_sung_line_else_the_track() {
        let mut ui = Ui::demo(0);
        assert_eq!(ui.mini_lines(), ("眠れないなら 一緒に歩こう".to_string(), "kotoza demo \u{b7} hoshi no ito".to_string()));
        ui.lyrics = Job::Failed(String::new());
        assert_eq!(ui.mini_lines(), ("hoshi no ito".to_string(), "kotoza demo \u{b7} no lyrics for this one..".to_string()));
        ui.st.len = 0.0;
        assert_eq!(ui.mini_lines().0, "fetching..");
    }

    #[test]
    fn both_pages_draw() {
        for page in [0, 1] {
            let mut ui = Ui::demo(page);
            let cv = ui.render(1440, 900, 1.0);
            assert!(cv.px.iter().any(|p| p >> 24 == 255), "page {page} drew nothing");
        }
    }
}
