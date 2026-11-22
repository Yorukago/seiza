//! The page about yoru and seiza. Everything on it comes from `~/.config/seiza/about`, which yoru writes:
//! nothing about the characters is built into the program.
//!
//! The file is plain text, one card per `[name]` line, then `key: value` rows under it:
//!
//! ```text
//! # a comment
//! [name]
//! key: value
//! a longer value
//!   can go on indented lines
//! ```
//!
//! A line with no `key:` becomes a plain note row. Lines before the first `[name]` are ignored.
//!
//! This part is the parser and the wording of the empty page, which need no drawing to test.

use std::{path::PathBuf, time::SystemTime};

use super::{Fonts, Page};
use seiza_common::{
    draw::Canvas,
    kit,
    palette::{palette, rgba},
};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Card {
    pub name: String,
    pub rows: Vec<(String, String)>, // (key, value); an empty key is a note
}

pub fn path() -> PathBuf {
    seiza_common::config::path("about")
}

/// What the page says when the file is missing or has no cards: where to put it and its shape.
pub const EMPTY_NOTE: &str = "nothing here yet..";
pub const EMPTY_HOW: &str = "write ~/.config/seiza/about and this page fills in: a [name] line starts a card, and key: value lines under it are its rows.";
pub const EXAMPLE: [&str; 4] = ["[name]", "key: value", "another key: another value", "a line with no key is just a note"];

pub fn parse(text: &str) -> Vec<Card> {
    let mut cards: Vec<Card> = vec![];
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            cards.push(Card { name: name.trim().to_string(), rows: vec![] });
            continue;
        }
        let Some(card) = cards.last_mut() else { continue };
        let indented = raw.starts_with([' ', '\t']);
        match (indented, card.rows.last_mut(), line.split_once(':')) {
            (true, Some(last), _) => {
                last.1.push(' ');
                last.1.push_str(line); // an indented line carries on the row above
            }
            (_, _, Some((k, v))) if !k.trim().is_empty() => card.rows.push((k.trim().to_string(), v.trim().to_string())),
            _ => card.rows.push((String::new(), line.to_string())),
        }
    }
    cards
}

/// (name, description) pairs for the help search: "yoru: likes" -> its value, and a card's notes under its name.
pub fn search_rows(cards: &[Card]) -> Vec<(String, String)> {
    cards
        .iter()
        .flat_map(|c| c.rows.iter().map(move |(k, v)| (if k.is_empty() { c.name.clone() } else { format!("{}: {k}", c.name) }, v.clone())))
        .collect()
}

pub struct Oc {
    path: PathBuf,
    cards: Vec<Card>,
    stamp: Option<SystemTime>, // when the file was last read, so an edit shows up by itself
}

impl Oc {
    pub fn new() -> Oc {
        Oc::at(path())
    }

    pub fn at(path: PathBuf) -> Oc {
        let mut oc = Oc { path, cards: vec![], stamp: None };
        oc.reload();
        oc
    }

    fn modified(&self) -> Option<SystemTime> {
        std::fs::metadata(&self.path).and_then(|m| m.modified()).ok()
    }

    fn reload(&mut self) {
        self.stamp = self.modified();
        self.cards = std::fs::read_to_string(&self.path).map(|t| parse(&t)).unwrap_or_default();
    }
}

const GAP: f32 = 14.0;
const KEY_W: f32 = 120.0;
const LINE: f32 = 20.0;

/// A row's value broken into lines for `room` pixels.
fn lines(f: &mut Fonts, value: &str, room: f32) -> Vec<String> {
    let out = kit::wrap(f.body, value, room.max(40.0));
    if out.is_empty() { vec![String::new()] } else { out }
}

impl Page for Oc {
    fn name(&self) -> &'static str {
        "yoru & seiza"
    }

    fn blurb(&self) -> String {
        if self.cards.is_empty() { "nothing here yet.. it fills in from ~/.config/seiza/about.".into() } else { "from ~/.config/seiza/about: edit it and this page updates.".into() }
    }

    fn rows(&self) -> Vec<(String, String)> {
        search_rows(&self.cards)
    }

    fn tick(&mut self) -> bool {
        if self.modified() == self.stamp {
            return false;
        }
        self.reload();
        true
    }

    fn draw(&mut self, cv: &mut Canvas, f: &mut Fonts, scroll: f32) -> f32 {
        let (w, p) = (cv.w as f32, palette());
        let cols = if w >= 900.0 { 2 } else { 1 };
        let cw = (w - GAP * (cols - 1) as f32) / cols as f32;

        // what each card says, as lines: (key, wrapped value lines), and how tall the card is
        let laid: Vec<(Vec<(String, Vec<String>)>, f32)> = self
            .cards
            .iter()
            .map(|card| {
                let rows: Vec<_> = card
                    .rows
                    .iter()
                    .map(|(k, v)| {
                        let room = cw - 36.0 - if k.is_empty() { 0.0 } else { KEY_W + 12.0 };
                        (k.clone(), lines(f, v, room))
                    })
                    .collect();
                let body: f32 = rows.iter().map(|(_, l)| l.len() as f32 * LINE + 8.0).sum();
                (rows, kit::CARD_HEAD + 12.0 + body + 10.0)
            })
            .collect();

        // each card goes under the shorter column
        let mut col_y = vec![0.0_f32; cols];
        let mut spots = vec![];
        for (_, h) in &laid {
            let c = (0..cols).min_by(|&a, &b| col_y[a].total_cmp(&col_y[b])).unwrap();
            spots.push((c as f32 * (cw + GAP), col_y[c]));
            col_y[c] += h + GAP;
        }
        let empty_h = if self.cards.is_empty() { 230.0 } else { 0.0 };
        let height = (col_y.iter().cloned().fold(0.0, f32::max) + empty_h).max(empty_h).ceil().max(1.0);
        let mut c = Canvas { w: cv.w, h: height as usize, px: vec![0; cv.w * height as usize] };

        if self.cards.is_empty() {
            let w2 = w.min(760.0);
            kit::card(&mut c, f.small, 0.0, 0.0, w2, empty_h, EMPTY_NOTE);
            let how = kit::wrap(f.body, EMPTY_HOW, w2 - 36.0);
            let mut y = kit::CARD_HEAD + 28.0;
            for l in &how {
                f.body.draw(&mut c, 18.0, y, l, rgba(p.text, 0.8));
                y += LINE;
            }
            y += 6.0;
            f.small.draw(&mut c, 18.0, y + 8.0, "like this:", rgba(p.text, 0.5));
            let box_h = EXAMPLE.len() as f32 * 18.0 + 16.0;
            c.round_rect(18.0, y + 18.0, w2 - 36.0, box_h, 10.0, rgba(p.text, 0.1));
            c.round_rect(19.0, y + 19.0, w2 - 38.0, box_h - 2.0, 9.0, rgba(p.sky, 0.9));
            for (i, l) in EXAMPLE.iter().enumerate() {
                f.small.draw(&mut c, 32.0, y + 18.0 + 20.0 + i as f32 * 18.0, l, rgba(p.text, 0.7));
            }
        }

        for ((card, (rows, h)), (x, y)) in self.cards.iter().zip(&laid).zip(spots) {
            kit::card(&mut c, f.small, x, y, cw, *h, &card.name);
            let mut ry = y + kit::CARD_HEAD + 30.0;
            for (key, ls) in rows {
                let vx = if key.is_empty() { x + 18.0 } else { x + 18.0 + KEY_W + 12.0 };
                if !key.is_empty() {
                    f.small.draw(&mut c, x + 18.0, ry, key, rgba(p.text, 0.5));
                }
                for (i, l) in ls.iter().enumerate() {
                    f.body.draw(&mut c, vx, ry + i as f32 * LINE, l, rgba(p.text, if key.is_empty() { 0.75 } else { 0.95 }));
                }
                ry += ls.len() as f32 * LINE + 8.0;
            }
        }

        cv.over(&c, 0, -(scroll as i32));
        height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "ignored before any card\n# a comment\n[First]\nkey: value one\n  continues here\nother key:  spaced  \na plain note\n\n[Second one]\nlikes: a: b\n: no key\n";

    #[test]
    fn cards_rows_and_continuations() {
        let c = parse(SAMPLE);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].name, "First");
        assert_eq!(c[0].rows, [("key".into(), "value one continues here".into()), ("other key".into(), "spaced".into()), (String::new(), "a plain note".into())]);
        assert_eq!(c[1].name, "Second one");
        assert_eq!(c[1].rows[0], ("likes".to_string(), "a: b".to_string()), "only the first colon splits");
        assert_eq!(c[1].rows[1], (String::new(), ": no key".to_string()), "an empty key is a note");
    }

    #[test]
    fn nothing_parses_to_nothing() {
        assert!(parse("").is_empty());
        assert!(parse("just text\nmore: text").is_empty(), "no card to put them in");
        assert_eq!(parse("[empty]"), [Card { name: "empty".into(), rows: vec![] }]);
    }

    #[test]
    fn search_rows_name_their_card() {
        let rows = search_rows(&parse(SAMPLE));
        assert_eq!(rows[0], ("First: key".to_string(), "value one continues here".to_string()));
        assert_eq!(rows[2], ("First".to_string(), "a plain note".to_string()));
        assert_eq!(rows.len(), 5);
    }

    #[test]
    fn the_file_lives_in_the_seiza_config_dir() {
        assert!(path().ends_with("seiza/about"));
        assert!(EMPTY_HOW.contains("~/.config/seiza/about"));
    }

    fn draw(page: &mut Oc, w: usize, h: usize) -> (Canvas, f32) {
        use seiza_common::draw::{shared_font, Text};
        let font = shared_font();
        let (mut t, mut b, mut s) = (Text::new(font.clone(), 24.0), Text::new(font.clone(), 15.0), Text::new(font, 13.0));
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let height = page.draw(&mut cv, &mut Fonts { title: &mut t, body: &mut b, small: &mut s }, 0.0);
        (cv, height)
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("seiza-oc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("about")
    }

    #[test]
    fn a_missing_file_gets_a_friendly_note_and_nothing_made_up() {
        let mut page = Oc::at(temp("missing"));
        assert!(page.rows().is_empty() && page.blurb().contains("~/.config/seiza/about"));
        let (cv, h) = draw(&mut page, 1420, 700);
        assert!(h > 100.0 && cv.px.iter().any(|&p| p != 0));
        assert!(!page.tick(), "still missing: no change");
    }

    #[test]
    fn cards_from_the_file_draw_in_columns_and_are_searchable() {
        let path = temp("cards");
        std::fs::write(&path, "[One]\nkey: a value\nsome note that is long enough to wrap onto a second line when the card is narrow, and then a third line as well, just to be sure\n[Two]\nother: thing\n").unwrap();
        let mut page = Oc::at(path.clone());
        assert_eq!(page.cards.len(), 2);
        assert_eq!(page.rows().len(), 3);
        let (_, wide) = draw(&mut page, 1420, 700);
        let (_, narrow) = draw(&mut page, 500, 700);
        assert!(narrow > wide, "one column is taller than two: {narrow} vs {wide}");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn editing_the_file_updates_the_page() {
        let path = temp("edit");
        std::fs::write(&path, "[One]\nk: v\n").unwrap();
        let mut page = Oc::at(path.clone());
        assert!(!page.tick(), "nothing changed yet");
        std::fs::write(&path, "[One]\nk: v\n[Two]\nk: w\n").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
        assert!(page.tick(), "the edit shows up");
        assert_eq!(page.cards.len(), 2);
        assert!(!page.tick());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
