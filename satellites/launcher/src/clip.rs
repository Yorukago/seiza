//! The clipboard picker (`seiza-launcher --clip`, Super+v): pinned and recent copies on the left, the whole
//! text of the selected one on the right, with paste / pin / forget. No Wayland in here, so `--dump` and the
//! tests run it anywhere.
//!
//! History lives in $XDG_RUNTIME_DIR/seiza-clip, one file per copy named by its millisecond stamp.
//! `<ms>.txt` is recent; pinning renames it to `<ms>.pin`, which scripts/seiza-clip-store never prunes.
//! Both are on tmpfs, so a pin is gone at logout like everything else here: a pinned password never
//! reaches the disk.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use seiza_common::{
    draw::{composite, shared_font, Canvas, Rgba, Text},
    kit::{pill_button, window_plate},
    palette::{palette, rgba},
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Note,
    Command,
    Link,
}

pub struct Entry {
    pub path: PathBuf,
    pub text: String,
    pub kind: Kind,
    pub ms: u64,
    pub pinned: bool,
}

/// What is under a point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Outside, // off the panel: a click there closes it
    Inside,
    Row(usize),
    Paste,
    Pin,
    Forget,
}

type Rect = (f32, f32, f32, f32);

const HEAD_H: f32 = 88.0;
const FOOT_H: f32 = 50.0;
const ROW_H: f32 = 56.0;
const SECTION_H: f32 = 34.0;
const GLIDE: Duration = Duration::from_millis(140);

/// Words a shell understands without a program behind them.
const BUILTINS: [&str; 12] = ["cd", "export", "alias", "source", "for", "while", "if", "case", "set", "unset", "function", "exec"];

fn on_path(cmd: &str) -> bool {
    if BUILTINS.contains(&cmd) || cmd.starts_with("./") || cmd.starts_with('/') || cmd.starts_with("~/") {
        return true;
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

/// A link, a shell command or a note.
// shortcut: a command is "first word is a program on PATH, and the line is short or has shell punctuation";
// prose that happens to start with a program name and contains a dash is called a command. Fine for an icon.
pub fn classify(text: &str) -> Kind {
    let t = text.trim();
    if (t.starts_with("http://") || t.starts_with("https://")) && !t.contains(char::is_whitespace) {
        return Kind::Link;
    }
    let first = t.lines().next().unwrap_or("");
    let mut words = first.split_whitespace().peekable();
    while words.next_if(|w| ["sudo", "doas", "env"].contains(w)).is_some() {}
    let shellish = first.contains(|c| "-/|&>$=~;*".contains(c));
    match words.next() {
        Some(cmd) if on_path(cmd) && (first.split_whitespace().count() <= 2 || shellish) => Kind::Command,
        _ => Kind::Note,
    }
}

fn sort(v: &mut [Entry]) {
    v.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.ms.cmp(&a.ms)));
}

/// The history in `dir`: pinned first, then recent, each newest first.
pub fn load(dir: &Path) -> Vec<Entry> {
    let mut out: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let pinned = match path.extension()?.to_str()? {
                "txt" => false,
                "pin" => true,
                _ => return None, // the store's half-written temp files
            };
            let ms = path.file_stem()?.to_str()?.parse().ok()?;
            let text = std::fs::read_to_string(&path).ok()?;
            (!text.trim().is_empty()).then(|| Entry { kind: classify(&text), path, text, ms, pinned })
        })
        .collect();
    sort(&mut out);
    out
}

fn ago(secs: u64) -> String {
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// Shorten `s` with ".." until it fits in `max` pixels.
pub(crate) fn fit(t: &mut Text, s: &str, max: f32) -> String {
    if t.width(s) <= max {
        return s.to_string();
    }
    let mut out = s.to_string();
    while !out.is_empty() && t.width(&format!("{out}..")) > max {
        out.pop();
    }
    format!("{out}..")
}

/// `text` as lines no wider than `room`: broken at spaces, a word wider than the room is cut, blank lines
/// stay. At most `max` lines; if there was more, the last ends in "…".
fn wrap_text(t: &mut Text, text: &str, room: f32, max: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let text = text.replace('\t', "    ");
    'all: for src in text.trim_end().lines() {
        let mut cur = String::new();
        let indent = src.len() - src.trim_start_matches(' ').len(); // kept: it matters in code
        for (n, word) in src.trim_start_matches(' ').split(' ').enumerate() {
            let mut word = if n == 0 { format!("{}{word}", " ".repeat(indent)) } else { word.to_string() };
            loop {
                let joined = if cur.is_empty() { word.clone() } else { format!("{cur} {word}") };
                if t.width(&joined) <= room {
                    cur = joined;
                    break;
                }
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur)); // it does not fit on this line: start the next
                    if out.len() > max {
                        break 'all;
                    }
                    continue;
                }
                let (mut n, mut w) = (0, 0.0);
                for ch in word.chars() {
                    w += t.glyph(ch).0.advance_width;
                    if w > room {
                        break;
                    }
                    n += 1;
                }
                let n = n.max(1);
                out.push(word.chars().take(n).collect());
                if out.len() > max {
                    break 'all;
                }
                word = word.chars().skip(n).collect();
                if word.is_empty() {
                    break;
                }
            }
        }
        out.push(cur);
        if out.len() > max {
            break;
        }
    }
    if out.len() > max {
        out.truncate(max);
        if let Some(last) = out.last_mut() {
            let mut l = last.trim_end().to_string();
            while !l.is_empty() && t.width(&format!("{l}…")) > room {
                l.pop(); // make room for the ellipsis
            }
            *last = format!("{l}…");
        }
    }
    out
}

/// A rounded outline (a stadium) of `w` x `h` centred on (cx, cy), turned by `rot` radians.
fn stadium(cx: f32, cy: f32, w: f32, h: f32, rot: f32) -> Vec<(f32, f32)> {
    let (a, r) = ((w - h) / 2.0, h / 2.0);
    let mut pts: Vec<(f32, f32)> = vec![];
    for (centre, from) in [(a, -90.0f32), (-a, 90.0)] {
        for k in 0..=8 {
            let t = (from + 180.0 * k as f32 / 8.0).to_radians();
            pts.push((centre + r * t.cos(), r * t.sin()));
        }
    }
    pts.push(pts[0]);
    let (s, c) = rot.sin_cos();
    pts.into_iter().map(|(x, y)| (cx + x * c - y * s, cy + x * s + y * c)).collect()
}

fn icon(cv: &mut Canvas, kind: Kind, cx: f32, cy: f32, c: Rgba) {
    match kind {
        Kind::Note => {
            let (x0, y0, x1, y1) = (cx - 6.5, cy - 8.0, cx + 6.5, cy + 8.0);
            cv.polyline(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)], 1.4, c);
            for k in 0..3 {
                let y = cy - 3.5 + 3.5 * k as f32;
                cv.polyline(&[(cx - 3.5, y), (cx + 3.5, y)], 1.2, c);
            }
        }
        Kind::Command => {
            cv.polyline(&[(cx - 7.0, cy - 5.0), (cx - 1.5, cy), (cx - 7.0, cy + 5.0)], 1.6, c);
            cv.polyline(&[(cx + 1.0, cy + 5.5), (cx + 7.0, cy + 5.5)], 1.6, c);
        }
        Kind::Link => {
            let rot = -std::f32::consts::FRAC_PI_4;
            cv.polyline(&stadium(cx - 3.2, cy + 3.2, 12.0, 7.0, rot), 1.4, c);
            cv.polyline(&stadium(cx + 3.2, cy - 3.2, 12.0, 7.0, rot), 1.4, c);
        }
    }
}

enum Row {
    Section(&'static str),
    Item(usize), // index into `shown`
}

pub struct ClipUi {
    pub w: f32,
    pub h: f32,
    pub query: String,
    pub error: Option<String>,
    entries: Vec<Entry>,
    shown: Vec<usize>, // indexes into `entries` that match the query, in order
    sel: usize,        // index into `shown`
    scroll: f32,       // pixels of the list scrolled off the top
    glide: Option<(f32, Instant)>, // the highlight sliding: where it left from (a row's y) and when
    rev: u32,          // bumped by anything that changes what is drawn, so the content is redrawn only then
    content: Option<(String, Canvas)>,
    plate: Option<(usize, usize, Canvas)>,
    t17: Text,
    t14: Text,
    t13: Text,
}

impl ClipUi {
    pub fn new(entries: Vec<Entry>, w: f32, h: f32) -> ClipUi {
        let font = shared_font();
        let mut ui = ClipUi {
            w,
            h,
            query: String::new(),
            error: None,
            entries,
            shown: vec![],
            sel: 0,
            scroll: 0.0,
            glide: None,
            rev: 0,
            content: None,
            plate: None,
            t17: Text::new(font.clone(), 17.0),
            t14: Text::new(font.clone(), 14.0),
            t13: Text::new(font, 13.0),
        };
        ui.reshow(None);
        ui
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.shown.get(self.sel).map(|&i| &self.entries[i])
    }

    /// Recompute what matches the query. The selection stays on the entry at `keep`, else on the same position.
    fn reshow(&mut self, keep: Option<&Path>) {
        let q = self.query.trim().to_lowercase();
        self.shown = (0..self.entries.len()).filter(|&i| q.is_empty() || self.entries[i].text.to_lowercase().contains(&q)).collect();
        self.sel = keep
            .and_then(|p| self.shown.iter().position(|&i| self.entries[i].path == p))
            .unwrap_or(self.sel.min(self.shown.len().saturating_sub(1)));
        self.glide = None;
        self.rev += 1;
        self.keep_visible();
    }

    /// The query changed: back to the top.
    pub fn refresh(&mut self) {
        self.sel = 0;
        self.scroll = 0.0;
        self.reshow(None);
    }

    pub fn resize(&mut self, w: f32, h: f32) {
        (self.w, self.h) = (w, h);
        self.rev += 1;
        self.keep_visible();
    }

    pub fn type_str(&mut self, s: &str) {
        self.query.push_str(s);
        self.refresh();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refresh();
    }

    // ---- geometry ----

    fn panel(&self) -> Rect {
        let pw = (self.w * 0.72).clamp(640.0, 1040.0).min(self.w - 40.0);
        let ph = (self.h * 0.74).clamp(440.0, 640.0).min(self.h - 40.0);
        ((self.w - pw) / 2.0, (self.h - ph) / 2.0 - self.h * 0.02, pw, ph)
    }

    fn list_w(&self) -> f32 {
        self.panel().2 * 0.445
    }

    fn body_h(&self) -> f32 {
        self.panel().3 - HEAD_H - FOOT_H
    }

    /// The list's rows in list coordinates (before scrolling): what, top, height.
    fn rows(&self) -> Vec<(Row, f32, f32)> {
        let (mut out, mut y, mut last) = (vec![], 8.0, None);
        for (k, &i) in self.shown.iter().enumerate() {
            let pinned = self.entries[i].pinned;
            if last != Some(pinned) {
                out.push((Row::Section(if pinned { "pinned" } else { "recent" }), y, SECTION_H));
                y += SECTION_H;
                last = Some(pinned);
            }
            out.push((Row::Item(k), y, ROW_H));
            y += ROW_H;
        }
        out
    }

    fn content_h(&self) -> f32 {
        self.rows().last().map_or(0.0, |(_, y, h)| y + h + 8.0)
    }

    /// Top of the selected row, and of the heading above it when it is the first of its section.
    fn sel_span(&self) -> Option<(f32, f32)> {
        let rows = self.rows();
        let at = rows.iter().position(|(r, ..)| matches!(r, Row::Item(k) if *k == self.sel))?;
        let top = if at > 0 && matches!(rows[at - 1].0, Row::Section(_)) { rows[at - 1].1 } else { rows[at].1 };
        Some((top, rows[at].1 + rows[at].2))
    }

    fn keep_visible(&mut self) {
        if let Some((top, bottom)) = self.sel_span() {
            let bh = self.body_h();
            if top < self.scroll {
                self.scroll = top;
            } else if bottom > self.scroll + bh {
                self.scroll = bottom - bh;
            }
        }
        self.scroll = self.scroll.clamp(0.0, (self.content_h() - self.body_h()).max(0.0));
    }

    fn sel_y(&self) -> f32 {
        self.rows().iter().find(|(r, ..)| matches!(r, Row::Item(k) if *k == self.sel)).map_or(0.0, |r| r.1)
    }

    /// Where the highlight is now (its row's top), sliding from where it left.
    fn highlight_y(&self) -> f32 {
        let Some((from, t0)) = self.glide else { return self.sel_y() };
        let t = (t0.elapsed().as_secs_f32() / GLIDE.as_secs_f32()).min(1.0);
        from + (self.sel_y() - from) * (1.0 - (1.0 - t).powi(3))
    }

    pub fn gliding(&self) -> bool {
        self.glide.is_some_and(|(_, t0)| t0.elapsed() < GLIDE)
    }

    pub fn select(&mut self, k: usize) {
        if k != self.sel && k < self.shown.len() {
            self.glide = Some((self.highlight_y(), Instant::now()));
            self.sel = k;
            self.rev += 1;
            self.keep_visible();
        }
    }

    pub fn step(&mut self, d: i32) {
        if !self.shown.is_empty() {
            self.select((self.sel as i32 + d).clamp(0, self.shown.len() as i32 - 1) as usize);
        }
    }

    pub fn scroll_by(&mut self, d: i32) {
        self.scroll = (self.scroll + 40.0 * d as f32).clamp(0.0, (self.content_h() - self.body_h()).max(0.0));
        self.rev += 1;
    }

    /// The preview box, and the three buttons under it.
    fn right(&self) -> (Rect, [(Rect, Hit); 3]) {
        let (px, py, pw, ph) = self.panel();
        let x0 = px + self.list_w() + 24.0;
        let by = py + ph - FOOT_H - 56.0;
        let pin_x = x0 + 88.0 + 68.0;
        let buttons = [((x0, by, 88.0, 34.0), Hit::Paste), ((pin_x, by, 76.0, 34.0), Hit::Pin), ((pin_x + 86.0, by, 90.0, 34.0), Hit::Forget)];
        ((x0, py + HEAD_H + 58.0, px + pw - 24.0 - x0, by - 14.0 - (py + HEAD_H + 58.0)), buttons)
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let (px, py, pw, ph) = self.panel();
        if x < px || x >= px + pw || y < py || y >= py + ph {
            return Hit::Outside;
        }
        let (top, lw) = (py + HEAD_H, self.list_w());
        if x < px + lw && y >= top && y < top + self.body_h() {
            let cy = y - top + self.scroll;
            return self.rows().iter().find_map(|(r, ry, rh)| match r {
                Row::Item(k) if cy >= *ry && cy < ry + rh => Some(Hit::Row(*k)),
                _ => None,
            }).unwrap_or(Hit::Inside);
        }
        if self.selected().is_some() {
            for ((bx, by, bw, bh), what) in self.right().1 {
                if x >= bx && x < bx + bw && y >= by && y < by + bh {
                    return what;
                }
            }
        }
        Hit::Inside
    }

    // ---- what the buttons do ----

    pub fn toggle_pin(&mut self) {
        let Some(&i) = self.shown.get(self.sel) else { return };
        let e = &mut self.entries[i];
        let to = e.path.with_extension(if e.pinned { "txt" } else { "pin" });
        if let Err(err) = std::fs::rename(&e.path, &to) {
            self.error = Some(format!("couldn't pin that: {err}"));
            return;
        }
        e.path = to.clone();
        e.pinned = !e.pinned;
        sort(&mut self.entries);
        self.reshow(Some(&to));
    }

    pub fn forget(&mut self) {
        let Some(&i) = self.shown.get(self.sel) else { return };
        if let Err(err) = std::fs::remove_file(&self.entries[i].path) {
            self.error = Some(format!("couldn't forget that: {err}"));
            return;
        }
        self.entries.remove(i);
        self.reshow(None);
    }

    // ---- drawing ----

    fn put(t: &mut Text, size: f32, cv: &mut Canvas, x: f32, cy: f32, s: &str, c: Rgba) -> f32 {
        t.draw(cv, x, cy + size * 0.33, s, c)
    }

    /// One frame at visibility `e` (0 = gone, 1 = open): the content is drawn once per change and composited per frame.
    pub fn render(&mut self, e: f32) -> Canvas {
        let p = palette();
        let key = format!("{}|{}|{:.2}|{}x{}|{:?}", self.rev, self.sel, self.highlight_y(), self.w, self.h, self.error);
        if self.content.as_ref().is_none_or(|(k, _)| *k != key) {
            let (w, h) = (self.w as usize, self.h as usize);
            let mut cv = Canvas { w, h, px: vec![0; w * h] };
            self.draw(&mut cv);
            self.content = Some((key, cv));
        }
        let content = &self.content.as_ref().unwrap().1;
        let mut out = Canvas { w: content.w, h: content.h, px: vec![0; content.w * content.h] };
        let a = 0.86 * e; // a flat premultiplied scrim: a rounded-rect pass over the whole screen is far slower
        let ch = |v: f32| (v * a * 255.0) as u32;
        out.px.fill(((a * 255.0) as u32) << 24 | ch(p.sky[0]) << 16 | ch(p.sky[1]) << 8 | ch(p.sky[2]));
        let (px, py, pw, ph) = self.panel();
        composite(&mut out, content, (px + pw / 2.0, py + ph / 2.0), 0.9 + 0.1 * e, 0.0, e, Some((px - 60.0, py - 60.0, pw + 120.0, ph + 120.0)));
        out
    }

    fn draw(&mut self, cv: &mut Canvas) {
        let p = palette();
        let (px, py, pw, ph) = self.panel();
        let (text, dim) = (rgba(p.text, 1.0), rgba(p.text, 0.55));

        if self.plate.as_ref().is_none_or(|(w, h, _)| (*w, *h) != (cv.w, cv.h)) {
            self.plate = Some((cv.w, cv.h, window_plate(cv.w, cv.h, (px, py), (pw, ph), FOOT_H + 8.0)));
        }
        cv.over(&self.plate.as_ref().unwrap().2, 0, 0);

        // search row: a ring with a sparkle, then the text pill
        let cy = py + HEAD_H / 2.0;
        cv.round_rect(px + 24.0, cy - 19.0, 38.0, 38.0, 19.0, rgba(p.star, 0.35));
        cv.round_rect(px + 25.0, cy - 18.0, 36.0, 36.0, 18.0, rgba(p.panel, 1.0));
        cv.sparkle(px + 43.0, cy, 8.0, rgba(p.star, 1.0));
        let pill = (px + 76.0, pw - 100.0);
        cv.round_rect(pill.0, cy - 22.0, pill.1, 44.0, 22.0, rgba(p.star, 0.28));
        cv.round_rect(pill.0 + 1.0, cy - 21.0, pill.1 - 2.0, 42.0, 21.0, rgba(p.sky, 0.9));
        if self.query.is_empty() {
            Self::put(&mut self.t17, 17.0, cv, pill.0 + 24.0, cy, "search the sky..", rgba(p.text, 0.35));
            cv.round_rect(pill.0 + 22.0, cy - 9.0, 1.5, 18.0, 0.0, rgba(p.text, 0.9));
        } else {
            let max = pill.1 - 60.0;
            let mut shown: &str = &self.query; // too long to fit: its end, where the typing is
            while self.t17.width(shown) > max {
                shown = &shown[shown.chars().next().map_or(0, char::len_utf8)..];
            }
            let end = Self::put(&mut self.t17, 17.0, cv, pill.0 + 24.0, cy, shown, text);
            cv.round_rect(end + 1.0, cy - 9.0, 1.5, 18.0, 0.0, rgba(p.text, 0.9));
        }
        cv.round_rect(px, py + HEAD_H - 1.0, pw, 1.0, 0.0, rgba(p.text, 0.08));

        // the list, drawn on its own canvas so rows cannot spill over the header or footer
        let (lw, bh) = (self.list_w(), self.body_h());
        let mut list = Canvas { w: lw as usize, h: bh as usize, px: vec![0; lw as usize * bh as usize] };
        let rows = self.rows();
        if self.shown.is_empty() {
            let msg = if self.entries.is_empty() { "nothing copied yet.." } else { "nothing by that name.." };
            Self::put(&mut self.t17, 17.0, &mut list, 24.0, 38.0, msg, dim);
        } else {
            let hy = self.highlight_y() - self.scroll;
            list.round_rect(6.0, hy + 3.0, lw - 12.0, ROW_H - 6.0, 16.0, rgba(p.star, 0.4));
            list.round_rect(7.0, hy + 4.0, lw - 14.0, ROW_H - 8.0, 15.0, rgba(p.dust, 0.9));
        }
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        for (row, y, h) in rows {
            let y = y - self.scroll;
            if y + h < 0.0 || y > bh {
                continue;
            }
            match row {
                Row::Section(name) => {
                    Self::put(&mut self.t13, 13.0, &mut list, 24.0, y + SECTION_H / 2.0 + 2.0, name, rgba(p.text, 0.5));
                }
                Row::Item(k) => {
                    let e = &self.entries[self.shown[k]];
                    let cy = y + ROW_H / 2.0;
                    list.round_rect(16.0, cy - 18.0, 36.0, 36.0, 12.0, rgba(p.sky, 0.8));
                    icon(&mut list, e.kind, 34.0, cy, rgba(p.star, 1.0));
                    let age = if e.pinned { String::new() } else { ago(now_ms.saturating_sub(e.ms) / 1000) };
                    let aw = self.t13.width(&age);
                    let first = e.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
                    let label = fit(&mut self.t17, first, lw - 64.0 - 24.0 - if age.is_empty() { 0.0 } else { aw + 14.0 });
                    Self::put(&mut self.t17, 17.0, &mut list, 64.0, cy, &label, text);
                    Self::put(&mut self.t13, 13.0, &mut list, lw - 20.0 - aw, cy, &age, rgba(p.text, 0.5));
                }
            }
        }
        cv.over(&list, px as i32, (py + HEAD_H) as i32);

        // the preview and its buttons
        let rx = px + lw;
        cv.round_rect(rx, py + HEAD_H, 1.0, bh, 0.0, rgba(p.text, 0.08));
        let (x0, top) = (rx + 24.0, py + HEAD_H);
        if let Some(e) = self.selected() {
            let (kind, pinned, len, body) = (e.kind, e.pinned, e.text.chars().count(), e.text.clone());
            let name = match kind {
                Kind::Note => "note",
                Kind::Command => "command",
                Kind::Link => "link",
            };
            let nw = self.t13.width(name) + 24.0;
            cv.round_rect(x0, top + 14.0, nw, 26.0, 13.0, rgba(p.text, 0.28));
            cv.round_rect(x0 + 1.0, top + 15.0, nw - 2.0, 24.0, 12.0, rgba(p.panel, 1.0));
            Self::put(&mut self.t13, 13.0, cv, x0 + 12.0, top + 27.0, name, rgba(p.text, 0.85));
            let count = format!("{len} {}", if len == 1 { "character" } else { "characters" });
            Self::put(&mut self.t13, 13.0, cv, x0 + nw + 12.0, top + 27.0, &count, rgba(p.text, 0.5));
            if pinned {
                cv.sparkle(px + pw - 34.0, top + 27.0, 8.0, rgba(p.star, 1.0));
            }
            let ((bx, by, bw, bh2), buttons) = self.right();
            cv.round_rect(bx, by, bw, bh2, 16.0, rgba(p.text, 0.1));
            cv.round_rect(bx + 1.0, by + 1.0, bw - 2.0, bh2 - 2.0, 15.0, rgba(p.sky, 0.75));
            let rows = ((bh2 - 36.0) / 20.0).max(1.0) as usize;
            for (j, l) in wrap_text(&mut self.t14, &body, bw - 40.0, rows).iter().enumerate() {
                Self::put(&mut self.t14, 14.0, cv, bx + 20.0, by + 28.0 + 20.0 * j as f32, l, text);
            }
            for ((rect, what), label) in buttons.into_iter().zip(["paste", if pinned { "unpin" } else { "pin" }, "forget"]) {
                pill_button(cv, &mut self.t13, rect, label, what == Hit::Paste);
            }
            Self::put(&mut self.t13, 13.0, cv, buttons[0].0 .0 + 88.0 + 12.0, buttons[0].0 .1 + 17.0, "enter", rgba(p.text, 0.45));
        }

        // footer
        let fy = py + ph - FOOT_H / 2.0;
        match &self.error {
            Some(err) => Self::put(&mut self.t13, 13.0, cv, px + 24.0, fy, err, rgba(p.pink, 1.0)),
            None => Self::put(&mut self.t13, 13.0, cv, px + 24.0, fy, "up / down: drift  ·  enter: paste  ·  tab: pin  ·  esc: close", rgba(p.text, 0.45)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("seiza-clip-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample(d: &Path) {
        std::fs::write(d.join("1000.txt"), "old note").unwrap();
        std::fs::write(d.join("2000.txt"), "https://example.org/x").unwrap();
        std::fs::write(d.join("3000.txt"), "ls -la /tmp").unwrap();
        std::fs::write(d.join("500.pin"), "keep me").unwrap();
        std::fs::write(d.join(".new.abc123"), "half written").unwrap();
    }

    #[test]
    fn kinds() {
        assert_eq!(classify("https://example.org/a?b=c"), Kind::Link);
        assert_eq!(classify("see https://example.org"), Kind::Note);
        assert_eq!(classify("ls -la /tmp"), Kind::Command);
        assert_eq!(classify("sudo ls -la"), Kind::Command);
        assert_eq!(classify("for f in *.png; do\n  echo $f\ndone"), Kind::Command);
        assert_eq!(classify("remember: buy oat milk + cat food"), Kind::Note);
        assert_eq!(classify("time to go to the shop"), Kind::Note, "prose that starts with a program name");
    }

    #[test]
    fn pinned_first_then_newest_and_temp_files_are_ignored() {
        let d = dir("load");
        sample(&d);
        let e = load(&d);
        assert_eq!(e.iter().map(|e| (e.ms, e.pinned)).collect::<Vec<_>>(), [(500, true), (3000, false), (2000, false), (1000, false)]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn pinning_renames_the_file_and_moves_the_entry_up_with_the_selection() {
        let d = dir("pin");
        sample(&d);
        let mut ui = ClipUi::new(load(&d), 1920.0, 1080.0);
        ui.select(3); // 1000, the oldest recent
        assert_eq!(ui.selected().unwrap().text, "old note");
        ui.toggle_pin();
        assert!(d.join("1000.pin").exists() && !d.join("1000.txt").exists());
        assert_eq!((ui.sel, ui.selected().unwrap().text.as_str()), (0, "old note"), "pinned, so above the older pin (500), and still selected");
        ui.toggle_pin();
        assert!(d.join("1000.txt").exists() && !d.join("1000.pin").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn forgetting_deletes_the_file_and_keeps_a_valid_selection() {
        let d = dir("forget");
        sample(&d);
        let mut ui = ClipUi::new(load(&d), 1920.0, 1080.0);
        ui.select(3);
        ui.forget();
        assert!(!d.join("1000.txt").exists());
        assert_eq!((ui.shown.len(), ui.sel), (3, 2));
        for _ in 0..3 {
            ui.forget();
        }
        assert!(ui.selected().is_none() && ui.shown.is_empty());
        ui.forget(); // nothing left: no panic
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn typing_filters_the_whole_text() {
        let d = dir("filter");
        sample(&d);
        let mut ui = ClipUi::new(load(&d), 1920.0, 1080.0);
        ui.type_str("EXAMPLE");
        assert_eq!(ui.shown.len(), 1);
        assert_eq!(ui.selected().unwrap().kind, Kind::Link);
        ui.type_str("zzz");
        assert!(ui.selected().is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn clicks_land_on_rows_buttons_or_outside() {
        let d = dir("hit");
        sample(&d);
        let ui = ClipUi::new(load(&d), 1920.0, 1080.0);
        let (px, py, pw, _) = ui.panel();
        assert_eq!(ui.hit(2.0, 2.0), Hit::Outside);
        // the first row is under the "pinned" heading: 8 + 34 px down the list
        assert_eq!(ui.hit(px + 100.0, py + HEAD_H + 8.0 + SECTION_H + 10.0), Hit::Row(0));
        assert_eq!(ui.hit(px + 100.0, py + HEAD_H + 8.0 + 2.0), Hit::Inside, "a heading is not a row");
        for (rect, what) in ui.right().1 {
            assert_eq!(ui.hit(rect.0 + rect.2 / 2.0, rect.1 + rect.3 / 2.0), what);
        }
        assert_eq!(ui.hit(px + pw - 5.0, py + 5.0), Hit::Inside);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn long_text_wraps_cuts_and_keeps_blank_lines() {
        let mut t = Text::new(shared_font(), 14.0);
        let l = wrap_text(&mut t, "a b\n\nc", 200.0, 5);
        assert_eq!(l, ["a b", "", "c"]);
        assert_eq!(wrap_text(&mut t, "a\n  b\n\tc", 200.0, 5), ["a", "  b", "    c"], "indentation survives");
        let l = wrap_text(&mut t, &"word ".repeat(80), 200.0, 3);
        assert_eq!(l.len(), 3);
        assert!(l[2].ends_with('…'));
        let l = wrap_text(&mut t, &"x".repeat(200), 100.0, 10);
        assert!(l.len() > 1 && l.iter().all(|s| t.width(s) <= 100.0), "one long word is cut to fit");
    }

    #[test]
    fn it_draws_with_and_without_history() {
        let d = dir("draw");
        let mut empty = ClipUi::new(load(&d), 1280.0, 720.0);
        empty.render(1.0);
        sample(&d);
        let mut ui = ClipUi::new(load(&d), 1280.0, 720.0);
        let cv = ui.render(1.0);
        assert_eq!((cv.w, cv.h), (1280, 720));
        ui.step(1);
        ui.render(0.5);
        std::fs::remove_dir_all(d).unwrap();
    }
}
