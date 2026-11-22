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

use std::path::PathBuf;

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

/// The cards in the file, or none if it is missing or unreadable.
pub fn load() -> Vec<Card> {
    std::fs::read_to_string(path()).map(|t| parse(&t)).unwrap_or_default()
}

/// (name, description) pairs for the help search: "yoru: likes" -> its value, and a card's notes under its name.
pub fn search_rows(cards: &[Card]) -> Vec<(String, String)> {
    cards
        .iter()
        .flat_map(|c| c.rows.iter().map(move |(k, v)| (if k.is_empty() { c.name.clone() } else { format!("{}: {k}", c.name) }, v.clone())))
        .collect()
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
}
