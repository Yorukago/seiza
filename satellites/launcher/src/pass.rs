//! The vault card (`seiza-launcher --pass`): a read-only view of a KeePass vault. The model here has no
//! Wayland in it, so unlock, grouping and find are tested anywhere. All crypto is the `keepass` crate's.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use seiza_common::{
    draw::{Canvas, Text, composite, shared_font},
    kit::{pill_button, window_plate},
    palette::{palette, rgba},
};
use zeroize::Zeroize;

use crate::clip::{Hit, fit};
use keepass::{
    Database, DatabaseKey,
    db::{EntryRef, GroupRef, fields},
};

/// The space of an entry that sits directly under the vault's root.
const LOOSE: &str = "other";

#[derive(Clone, Debug)]
pub struct Login {
    pub title: String,
    pub user: String,
    pub url: String,
    pub space: String,
    password: String,
}

impl Login {
    pub fn password(&self) -> &str {
        &self.password
    }
}

/// The lowercased host of a url or bare address, without scheme, port, path or `www.`; empty if none.
pub fn host(url: &str) -> String {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let h = rest.split(['/', '?', '#']).next().unwrap_or("");
    let h = h.rsplit_once('@').map_or(h, |(_, h)| h);
    let h = h.split(':').next().unwrap_or("").to_lowercase();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

/// Read every login of the vault at `path`. Errors are readable and never contain the passphrase.
pub fn open(path: &Path, passphrase: &str) -> Result<Vec<Login>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let db = Database::open(&mut file, DatabaseKey::new().with_password(passphrase))
        .map_err(|_| "can't unlock the vault: wrong passphrase, or not a kdbx file".to_string())?;
    let mut out = Vec::new();
    let root = db.root();
    for e in root.entries() {
        out.push(login(&e, LOOSE));
    }
    for g in root.groups() {
        walk(&g, &g.name, &mut out);
    }
    Ok(out)
}

/// Every entry under `g`, nested groups included, all filed under the top-level `space`.
fn walk(g: &GroupRef<'_>, space: &str, out: &mut Vec<Login>) {
    out.extend(g.entries().map(|e| login(&e, space)));
    for sub in g.groups() {
        walk(&sub, space, out);
    }
}

fn login(e: &EntryRef<'_>, space: &str) -> Login {
    let get = |k: &str| e.get(k).unwrap_or("").to_string();
    Login { title: get(fields::TITLE), user: get(fields::USERNAME), url: get(fields::URL), space: space.to_string(), password: get(fields::PASSWORD) }
}

/// `wl-copy --sensitive` marks the copy; scripts/seiza-clip-store skips marked copies, so it stays out of the history.
fn copy_cmd() -> Command {
    let mut c = Command::new("wl-copy");
    c.arg("--sensitive");
    c
}

/// Runs detached, so it outlives the card. shortcut: clears whatever is on the clipboard after the delay, even
/// a later copy; upgrade if that bites.
fn clear_cmd(after: Duration) -> Command {
    let mut c = Command::new("sh");
    c.args(["-c", &format!("sleep {}; wl-copy --clear", after.as_secs())]);
    c
}

/// Put `text` on the clipboard (on stdin, never in argv) and, if asked, clear it after `clear_after`.
pub fn copy_secret(text: &str, clear_after: Option<Duration>) {
    if text.is_empty() {
        return;
    }
    if let Ok(mut child) = copy_cmd().stdin(Stdio::piped()).spawn() {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
        if let Some(after) = clear_after {
            let _ = clear_cmd(after).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        }
    }
}

/// One row of the card: every login of a site within a space.
#[derive(Debug, PartialEq)]
pub struct Row {
    pub host: String,
    pub space: String,
    pub logins: Vec<usize>,
}

/// Fold logins into one row per (space, host); a login without a host is a row of its own, named by its title.
pub fn fold(logins: &[Login]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for (i, l) in logins.iter().enumerate() {
        let h = host(&l.url);
        match rows.iter_mut().find(|r| !h.is_empty() && r.space == l.space && r.host == h) {
            Some(r) => r.logins.push(i),
            None => {
                let name = if !h.is_empty() { h } else if l.title.is_empty() { "(untitled)".into() } else { l.title.to_lowercase() };
                rows.push(Row { host: name, space: l.space.clone(), logins: vec![i] });
            }
        }
    }
    rows.sort_by(|a, b| (&a.space, &a.host).cmp(&(&b.space, &b.host)));
    rows
}

/// Indices into `rows` matching `query` (case-insensitive substring of title, user, host or space).
pub fn find(logins: &[Login], rows: &[Row], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    (0..rows.len())
        .filter(|&i| {
            let r = &rows[i];
            q.is_empty()
                || r.host.contains(&q)
                || r.space.to_lowercase().contains(&q)
                || r.logins.iter().any(|&j| logins[j].title.to_lowercase().contains(&q) || logins[j].user.to_lowercase().contains(&q))
        })
        .collect()
}


type Rect = (f32, f32, f32, f32);

const HEAD_H: f32 = 88.0;
const FOOT_H: f32 = 50.0;
const ROW_H: f32 = 52.0;
const SECTION_H: f32 = 34.0;
/// How long a copied password stays on the clipboard.
pub const CLEAR_AFTER: Duration = Duration::from_secs(20);

/// What pressing enter asks the caller to put on the clipboard.
#[derive(Debug, PartialEq)]
pub enum Enter {
    Nothing,
    User(String),
    Password(String),
}

enum Line {
    Section(String),
    Item(usize), // index into `shown`
}

pub struct PassUi {
    pub w: f32,
    pub h: f32,
    pub query: String,
    pub error: Option<String>,
    vault: PathBuf,
    typed: String, // the passphrase while locked; wiped the moment the vault opens
    unlocked: bool,
    logins: Vec<Login>,
    rows: Vec<Row>,
    shown: Vec<usize>, // indexes into `rows` that match the query
    sel: usize,        // index into `shown`
    acct: usize,       // which login of the selected row
    user_copied: bool, // the next enter copies the password
    scroll: f32,
    rev: u32,
    content: Option<(String, Canvas)>,
    plate: Option<(usize, usize, Canvas)>,
    t17: Text,
    t14: Text,
    t13: Text,
}

impl Drop for PassUi {
    fn drop(&mut self) {
        self.typed.zeroize();
        for l in &mut self.logins {
            l.password.zeroize();
        }
    }
}

impl PassUi {
    pub fn new(vault: PathBuf, w: f32, h: f32) -> PassUi {
        let font = shared_font();
        PassUi {
            w,
            h,
            query: String::new(),
            error: None,
            vault,
            typed: String::new(),
            unlocked: false,
            logins: vec![],
            rows: vec![],
            shown: vec![],
            sel: 0,
            acct: 0,
            user_copied: false,
            scroll: 0.0,
            rev: 0,
            content: None,
            plate: None,
            t17: Text::new(font.clone(), 17.0),
            t14: Text::new(font.clone(), 14.0),
            t13: Text::new(font, 13.0),
        }
    }

    /// What the prompt shows for the passphrase typed so far: dots, never the text.
    fn prompt_text(&self) -> String {
        "•".repeat(self.typed.chars().count())
    }

    pub fn unlocked(&self) -> bool {
        self.unlocked
    }

    fn selected_row(&self) -> Option<&Row> {
        self.shown.get(self.sel).map(|&i| &self.rows[i])
    }

    fn selected_login(&self) -> Option<&Login> {
        let row = self.selected_row()?;
        row.logins.get(self.acct).map(|&j| &self.logins[j])
    }

    pub fn type_str(&mut self, s: &str) {
        if self.unlocked {
            self.query.push_str(s);
            self.reshow();
        } else {
            self.typed.push_str(s);
            self.rev += 1;
        }
    }

    pub fn backspace(&mut self) {
        if self.unlocked {
            self.query.pop();
            self.reshow();
        } else {
            self.typed.pop();
            self.rev += 1;
        }
    }

    fn reshow(&mut self) {
        self.shown = find(&self.logins, &self.rows, &self.query);
        (self.sel, self.acct, self.user_copied, self.scroll) = (0, 0, false, 0.0);
        self.rev += 1;
    }

    pub fn resize(&mut self, w: f32, h: f32) {
        (self.w, self.h) = (w, h);
        self.rev += 1;
        self.keep_visible();
    }

    /// Escape: clears the search first; `false` means nothing was left to undo, so the card should close.
    pub fn back(&mut self) -> bool {
        if self.unlocked && !self.query.is_empty() {
            self.query.clear();
            self.reshow();
            return true;
        }
        false
    }

    pub fn select(&mut self, k: usize) {
        if k != self.sel && k < self.shown.len() {
            (self.sel, self.acct, self.user_copied) = (k, 0, false);
            self.rev += 1;
            self.keep_visible();
        }
    }

    pub fn step(&mut self, d: i32) {
        if !self.shown.is_empty() {
            self.select((self.sel as i32 + d).clamp(0, self.shown.len() as i32 - 1) as usize);
        }
    }

    /// Switch to the next or previous account of the selected site.
    pub fn cycle(&mut self, d: i32) {
        let n = self.selected_row().map_or(0, |r| r.logins.len());
        if n > 1 {
            self.acct = (self.acct as i32 + d).rem_euclid(n as i32) as usize;
            self.user_copied = false;
            self.rev += 1;
        }
    }

    /// Locked: try the typed passphrase. Open: ask for the username, then (on the second press) the password.
    pub fn enter(&mut self) -> Enter {
        if !self.unlocked {
            match open(&self.vault, &self.typed) {
                Ok(logins) => {
                    self.rows = fold(&logins);
                    self.logins = logins;
                    self.unlocked = true;
                    self.error = None;
                    self.reshow();
                }
                Err(e) => {
                    self.error = Some(e);
                    self.rev += 1;
                }
            }
            self.typed.zeroize();
            return Enter::Nothing;
        }
        let Some(l) = self.selected_login() else { return Enter::Nothing };
        let out = if self.user_copied { Enter::Password(l.password().to_string()) } else { Enter::User(l.user.clone()) };
        self.user_copied = !self.user_copied;
        self.rev += 1;
        out
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

    /// The list in list coordinates (before scrolling): what, top, height.
    fn lines(&self) -> Vec<(Line, f32, f32)> {
        let (mut out, mut y, mut last) = (vec![], 8.0, None::<&str>);
        for (k, &i) in self.shown.iter().enumerate() {
            let space = self.rows[i].space.as_str();
            if last != Some(space) {
                out.push((Line::Section(space.to_string()), y, SECTION_H));
                y += SECTION_H;
                last = Some(space);
            }
            out.push((Line::Item(k), y, ROW_H));
            y += ROW_H;
        }
        out
    }

    fn content_h(&self) -> f32 {
        self.lines().last().map_or(0.0, |(_, y, h)| y + h + 8.0)
    }

    fn keep_visible(&mut self) {
        let bh = self.body_h();
        let lines = self.lines();
        if let Some(at) = lines.iter().position(|(l, ..)| matches!(l, Line::Item(k) if *k == self.sel)) {
            let top = if at > 0 && matches!(lines[at - 1].0, Line::Section(_)) { lines[at - 1].1 } else { lines[at].1 };
            let bottom = lines[at].1 + lines[at].2;
            if top < self.scroll {
                self.scroll = top;
            } else if bottom > self.scroll + bh {
                self.scroll = bottom - bh;
            }
        }
        self.scroll = self.scroll.clamp(0.0, (self.content_h() - bh).max(0.0));
    }

    pub fn scroll_by(&mut self, d: i32) {
        self.scroll = (self.scroll + 40.0 * d as f32).clamp(0.0, (self.content_h() - self.body_h()).max(0.0));
        self.rev += 1;
    }

    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let (px, py, pw, ph) = self.panel();
        if x < px || x >= px + pw || y < py || y >= py + ph {
            return Hit::Outside;
        }
        let (top, lw) = (py + HEAD_H, self.list_w());
        if self.unlocked && x < px + lw && y >= top && y < top + self.body_h() {
            let cy = y - top + self.scroll;
            return self.lines().iter().find_map(|(l, ly, lh)| match l {
                Line::Item(k) if cy >= *ly && cy < ly + lh => Some(Hit::Row(*k)),
                _ => None,
            }).unwrap_or(Hit::Inside);
        }
        Hit::Inside
    }

    // ---- drawing ----

    fn put(t: &mut Text, size: f32, cv: &mut Canvas, x: f32, cy: f32, s: &str, c: seiza_common::draw::Rgba) -> f32 {
        t.draw(cv, x, cy + size * 0.33, s, c)
    }

    /// One frame at visibility `e` (0 = gone, 1 = open): the content is drawn once per change and composited per frame.
    pub fn render(&mut self, e: f32) -> Canvas {
        let p = palette();
        let key = format!("{}|{}x{}", self.rev, self.w, self.h);
        if self.content.as_ref().is_none_or(|(k, _)| *k != key) {
            let (w, h) = (self.w as usize, self.h as usize);
            let mut cv = Canvas { w, h, px: vec![0; w * h] };
            self.draw(&mut cv);
            self.content = Some((key, cv));
        }
        let content = &self.content.as_ref().unwrap().1;
        let mut out = Canvas { w: content.w, h: content.h, px: vec![0; content.w * content.h] };
        let a = 0.86 * e;
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

        // the pill: the passphrase as dots while locked, the search once open
        let cy = py + HEAD_H / 2.0;
        cv.round_rect(px + 24.0, cy - 19.0, 38.0, 38.0, 19.0, rgba(p.star, 0.35));
        cv.round_rect(px + 25.0, cy - 18.0, 36.0, 36.0, 18.0, rgba(p.panel, 1.0));
        cv.sparkle(px + 43.0, cy, 8.0, rgba(p.star, 1.0));
        let pill = (px + 76.0, pw - 100.0);
        cv.round_rect(pill.0, cy - 22.0, pill.1, 44.0, 22.0, rgba(p.star, 0.28));
        cv.round_rect(pill.0 + 1.0, cy - 21.0, pill.1 - 2.0, 42.0, 21.0, rgba(p.sky, 0.9));
        let (shown, hint) = if self.unlocked { (self.query.clone(), "search the vault..") } else { (self.prompt_text(), "passphrase..") };
        if shown.is_empty() {
            Self::put(&mut self.t17, 17.0, cv, pill.0 + 24.0, cy, hint, rgba(p.text, 0.35));
            cv.round_rect(pill.0 + 22.0, cy - 9.0, 1.5, 18.0, 0.0, rgba(p.text, 0.9));
        } else {
            let label = fit(&mut self.t17, &shown, pill.1 - 60.0);
            let end = Self::put(&mut self.t17, 17.0, cv, pill.0 + 24.0, cy, &label, text);
            cv.round_rect(end + 1.0, cy - 9.0, 1.5, 18.0, 0.0, rgba(p.text, 0.9));
        }
        cv.round_rect(px, py + HEAD_H - 1.0, pw, 1.0, 0.0, rgba(p.text, 0.08));

        let (lw, bh) = (self.list_w(), self.body_h());
        if !self.unlocked {
            let msg = self.error.clone().unwrap_or_else(|| "unlock the vault to see your logins".into());
            let c = if self.error.is_some() { rgba(p.pink, 1.0) } else { dim };
            let label = fit(&mut self.t17, &msg, pw - 80.0);
            Self::put(&mut self.t17, 17.0, cv, px + 40.0, py + HEAD_H + 50.0, &label, c);
            Self::put(&mut self.t13, 13.0, cv, px + 24.0, py + ph - FOOT_H / 2.0, "enter: unlock  ·  esc: close", rgba(p.text, 0.45));
            return;
        }

        // the list, on its own canvas so rows cannot spill over the header or footer
        let mut list = Canvas { w: lw as usize, h: bh as usize, px: vec![0; lw as usize * bh as usize] };
        if self.shown.is_empty() {
            let msg = if self.rows.is_empty() { "the vault is empty.." } else { "nothing by that name.." };
            Self::put(&mut self.t17, 17.0, &mut list, 24.0, 38.0, msg, dim);
        }
        for (line, y, h) in self.lines() {
            let y = y - self.scroll;
            if y + h < 0.0 || y > bh {
                continue;
            }
            match line {
                Line::Section(name) => {
                    Self::put(&mut self.t13, 13.0, &mut list, 24.0, y + SECTION_H / 2.0 + 2.0, &name, rgba(p.text, 0.5));
                }
                Line::Item(k) => {
                    let row = &self.rows[self.shown[k]];
                    if k == self.sel {
                        list.round_rect(6.0, y + 3.0, lw - 12.0, ROW_H - 6.0, 16.0, rgba(p.star, 0.4));
                        list.round_rect(7.0, y + 4.0, lw - 14.0, ROW_H - 8.0, 15.0, rgba(p.dust, 0.9));
                    }
                    let cy = y + ROW_H / 2.0;
                    let count = if row.logins.len() > 1 { format!("×{}", row.logins.len()) } else { String::new() };
                    let cw = self.t13.width(&count);
                    let label = fit(&mut self.t17, &row.host, lw - 40.0 - cw - 24.0);
                    Self::put(&mut self.t17, 17.0, &mut list, 24.0, cy, &label, text);
                    Self::put(&mut self.t13, 13.0, &mut list, lw - 20.0 - cw, cy, &count, rgba(p.text, 0.5));
                }
            }
        }
        cv.over(&list, px as i32, (py + HEAD_H) as i32);

        // the selected site's accounts: never the password itself
        let rx = px + lw;
        cv.round_rect(rx, py + HEAD_H, 1.0, bh, 0.0, rgba(p.text, 0.08));
        let (x0, top) = (rx + 24.0, py + HEAD_H);
        if let Some(row) = self.selected_row() {
            let (host, space, accts) = (row.host.clone(), row.space.clone(), row.logins.clone());
            let label = fit(&mut self.t17, &host, pw - lw - 48.0);
            Self::put(&mut self.t17, 17.0, cv, x0, top + 28.0, &label, text);
            Self::put(&mut self.t13, 13.0, cv, x0, top + 52.0, &space, rgba(p.text, 0.5));
            for (n, &j) in accts.iter().enumerate() {
                let y = top + 84.0 + 46.0 * n as f32;
                if n == self.acct {
                    cv.round_rect(x0 - 8.0, y - 4.0, pw - lw - 32.0, 40.0, 14.0, rgba(p.dust, 0.9));
                }
                let l = &self.logins[j];
                let name = if l.user.is_empty() { l.title.as_str() } else { l.user.as_str() };
                let name = fit(&mut self.t14, name, pw - lw - 72.0);
                Self::put(&mut self.t14, 14.0, cv, x0 + 4.0, y + 16.0, &name, text);
            }
            let by = py + ph - FOOT_H - 56.0;
            let step = if self.user_copied { "copy password" } else { "copy username" };
            pill_button(cv, &mut self.t13, (x0, by, 130.0, 34.0), step, true);
        }
        let fy = py + ph - FOOT_H / 2.0;
        let help = "up / down: drift  ·  tab: next account  ·  enter: copy  ·  esc: close";
        match &self.error {
            Some(err) => Self::put(&mut self.t13, 13.0, cv, px + 24.0, fy, err, rgba(p.pink, 1.0)),
            None => Self::put(&mut self.t13, 13.0, cv, px + 24.0, fy, help, rgba(p.text, 0.45)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keepass::db::GroupMut;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PASS: &str = "correct horse battery";

    fn vault() -> std::path::PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("seiza-pass-test-{}-{}.kdbx", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let mut db = Database::new();
        let mut root = db.root_mut();
        let add = |g: &mut GroupMut<'_>, title: &str, user: &str, url: &str, pw: &str| {
            let mut e = g.add_entry();
            e.set_unprotected(fields::TITLE, title);
            e.set_unprotected(fields::USERNAME, user);
            e.set_unprotected(fields::URL, url);
            e.set_protected(fields::PASSWORD, pw);
        };
        let mut socials = root.add_group();
        socials.name = "socials".into();
        add(&mut socials, "gmail a", "a@x.org", "https://Mail.Google.com/x", "pw-a");
        add(&mut socials, "gmail b", "b@x.org", "mail.google.com", "pw-b");
        let mut deep = socials.add_group();
        deep.name = "old".into();
        add(&mut deep, "forum", "me", "https://forum.example/login", "pw-f");
        let mut fin = root.add_group();
        fin.name = "finances".into();
        add(&mut fin, "bank", "me", "https://www.bank.example:8443/", "");
        add(&mut fin, "", "nourl", "", "pw-n");
        add(&mut root, "loose", "l", "", "pw-l");
        let mut f = std::fs::File::create(&path).unwrap();
        db.save(&mut f, DatabaseKey::new().with_password(PASS)).unwrap();
        path
    }

    #[test]
    fn secret_commands_are_sensitive_and_hold_no_text() {
        let args = |c: &Command| c.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(args(&copy_cmd()), ["--sensitive"]);
        assert_eq!(args(&clear_cmd(Duration::from_secs(20))), ["-c", "sleep 20; wl-copy --clear"]);
    }

    #[test]
    fn host_strips_scheme_path_www_and_case() {
        assert_eq!(host("https://Mail.Google.com/x?y#z"), "mail.google.com");
        assert_eq!(host("www.Bank.example:8443/a"), "bank.example");
        assert_eq!(host("user@host.org"), "host.org");
        assert_eq!(host(""), "");
    }

    #[test]
    fn open_lists_logins_with_top_level_space() {
        let l = open(&vault(), PASS).unwrap();
        assert_eq!(l.len(), 6);
        let by = |t: &str| l.iter().find(|x| x.title == t).unwrap();
        assert_eq!(by("gmail a").space, "socials");
        assert_eq!(by("forum").space, "socials", "nested group belongs to its top-level space");
        assert_eq!(by("loose").space, "other");
        assert_eq!(by("gmail a").password(), "pw-a");
        assert_eq!(by("bank").password(), "", "an empty password stays listed");
    }

    #[test]
    fn open_wrong_passphrase_is_err_without_passphrase_in_message() {
        let e = open(&vault(), "nope-secret-guess").unwrap_err();
        assert!(e.contains("can't unlock"), "{e}");
        assert!(!e.contains("nope-secret-guess"), "{e}");
    }

    #[test]
    fn open_missing_file_names_the_path() {
        let e = open(Path::new("/nonexistent/v.kdbx"), PASS).unwrap_err();
        assert!(e.contains("/nonexistent/v.kdbx"), "{e}");
    }

    #[test]
    fn fold_merges_same_host_in_same_space() {
        let l = open(&vault(), PASS).unwrap();
        let rows = fold(&l);
        let g = rows.iter().find(|r| r.host == "mail.google.com").unwrap();
        assert_eq!((g.space.as_str(), g.logins.len()), ("socials", 2));
    }

    #[test]
    fn fold_keeps_urlless_entries_separate() {
        let l = open(&vault(), PASS).unwrap();
        let rows = fold(&l);
        assert!(rows.iter().any(|r| r.space == "other" && r.logins.len() == 1));
        assert!(rows.iter().any(|r| r.space == "finances" && r.host == "(untitled)"), "an untitled, urlless login still has a row");
        assert_eq!(rows.iter().map(|r| r.logins.len()).sum::<usize>(), 6);
    }

    #[test]
    fn find_matches_title_user_host_space_case_insensitively() {
        let l = open(&vault(), PASS).unwrap();
        let rows = fold(&l);
        let hits = |q: &str| find(&l, &rows, q).len();
        assert_eq!(hits(""), rows.len());
        assert_eq!(hits("GMAIL"), 1);
        assert_eq!(hits("b@x"), 1);
        assert_eq!(hits("finances"), 2);
        assert_eq!(hits("zzz"), 0);
    }

    fn ui_with(pass: &str) -> PassUi {
        let mut ui = PassUi::new(vault(), 1920.0, 1080.0);
        ui.type_str(pass);
        ui
    }

    #[test]
    fn locked_shows_error_and_stays_locked_on_wrong_passphrase() {
        let mut ui = ui_with("zq-not-it");
        assert_eq!(ui.enter(), Enter::Nothing);
        assert!(!ui.unlocked());
        assert!(ui.error.as_deref().is_some_and(|e| e.contains("can't unlock") && !e.contains("zq-not-it")));
        assert!(ui.typed.is_empty(), "the failed attempt is wiped too");
    }

    #[test]
    fn unlock_lists_spaces_and_wipes_the_passphrase() {
        let mut ui = ui_with(PASS);
        ui.enter();
        assert!(ui.unlocked() && ui.typed.is_empty());
        assert_eq!(ui.shown.len(), ui.rows.len());
        ui.type_str("gmail");
        assert_eq!(ui.shown.len(), 1);
    }

    #[test]
    fn enter_copies_username_then_password_and_switching_account_starts_over() {
        let mut ui = ui_with(PASS);
        ui.enter();
        ui.type_str("gmail");
        assert_eq!(ui.selected_row().unwrap().logins.len(), 2);
        let first = ui.selected_login().unwrap().user.clone();
        assert_eq!(ui.enter(), Enter::User(first.clone()));
        ui.cycle(1);
        assert_ne!(ui.selected_login().unwrap().user, first);
        let second = ui.selected_login().unwrap().user.clone();
        assert_eq!(ui.enter(), Enter::User(second.clone()), "a new account asks for its username first");
        assert!(matches!(ui.enter(), Enter::Password(pw) if pw.starts_with("pw-")));
    }

    #[test]
    fn enter_on_empty_results_does_nothing() {
        let mut ui = ui_with(PASS);
        ui.enter();
        ui.type_str("zzzz");
        assert_eq!(ui.enter(), Enter::Nothing);
    }

    #[test]
    fn back_clears_the_query_then_asks_to_close() {
        let mut ui = ui_with(PASS);
        ui.enter();
        ui.type_str("gm");
        assert!(ui.back());
        assert!(ui.query.is_empty());
        assert!(!ui.back());
    }

    #[test]
    fn typed_passphrase_is_never_in_the_prompt_text() {
        let ui = ui_with("hunter2");
        assert_eq!(ui.prompt_text(), "•••••••");
    }

    #[test]
    fn renders_every_state_without_panicking() {
        let mut ui = ui_with("x");
        ui.render(1.0);
        ui.enter();
        ui.render(1.0);
        ui.backspace();
        ui.type_str(PASS);
        ui.enter();
        let cv = ui.render(1.0);
        assert_eq!((cv.w, cv.h), (1920, 1080));
        ui.type_str("gmail");
        ui.render(0.5);
    }
}
