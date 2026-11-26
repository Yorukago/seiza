//! Lyrics from lrclib: timed lines when it has them, plain ones when it does not.
use crate::search::Track;
use serde_json::Value;
use std::process::Command;

#[derive(Debug, PartialEq, Clone)]
pub struct Lyrics {
    pub synced: bool,
    pub lines: Vec<(f32, String)>, // seconds and the line; all 0 when not synced
}

impl Lyrics {
    /// The line sung at `pos`: the last one whose time has come.
    pub fn current(&self, pos: f32) -> Option<usize> {
        if !self.synced {
            return None;
        }
        self.lines.iter().rposition(|(t, _)| *t <= pos + 0.15)
    }
}

/// A YouTube title as a search for a song: no brackets, no slashes.
pub fn clean(title: &str) -> String {
    let (mut out, mut depth) = (String::new(), 0);
    for c in title.chars() {
        match c {
            '(' | '[' | '【' | '（' | '「' | '『' => depth += 1,
            ')' | ']' | '】' | '）' | '」' | '』' => depth = (depth - 1).max(0),
            '/' | '|' | '～' => out.push(' '),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What to ask lrclib, best guess first: the cleaned title, then the title without "feat. ..." plus the artist
/// (lrclib files many songs under the bare title, and a video's length often matches only those).
fn queries(t: &Track) -> Vec<String> {
    let first = clean(&t.title);
    let lower = first.to_lowercase();
    let cut = [" feat.", " feat ", " ft.", " ft "].iter().filter_map(|m| lower.find(m)).min().unwrap_or(first.len());
    let mut second = first[..cut].trim().to_string();
    if !t.artist.is_empty() && !second.to_lowercase().contains(&t.artist.to_lowercase()) {
        second = format!("{second} {}", t.artist);
    }
    if second == first { vec![first] } else { vec![first, second] }
}

/// `[01:02.50] words` lines (and `[t][t] words`); a line with no words is kept: it is a pause.
fn parse_lrc(s: &str) -> Vec<(f32, String)> {
    let mut out = vec![];
    for line in s.lines() {
        let mut rest = line.trim();
        let mut times = vec![];
        while let Some(tag) = rest.strip_prefix('[').and_then(|r| r.split_once(']')) {
            let t = tag.0.split_once(':').and_then(|(m, s)| Some(m.parse::<f32>().ok()? * 60.0 + s.parse::<f32>().ok()?));
            match t {
                Some(t) => times.push(t),
                None => break, // [ar:...] and the like
            }
            rest = tag.1.trim_start();
        }
        out.extend(times.into_iter().map(|t| (t, rest.trim().to_string())));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// The best of lrclib's answers for a track `secs` long: one of about that length, timed ones first.
fn pick(json: &str, secs: u64) -> Option<Lyrics> {
    let all: Vec<Value> = serde_json::from_str(json).ok()?;
    let best = all
        .iter()
        .filter(|o| o["duration"].as_f64().is_some_and(|d| secs == 0 || (d - secs as f64).abs() <= 5.0))
        .filter(|o| o["syncedLyrics"].as_str().is_some_and(|s| !s.is_empty()) || o["plainLyrics"].as_str().is_some_and(|s| !s.is_empty()))
        .min_by_key(|o| (o["syncedLyrics"].as_str().is_none_or(str::is_empty), ((o["duration"].as_f64().unwrap_or(0.0) - secs as f64).abs() * 10.0) as u32))?;
    match best["syncedLyrics"].as_str().filter(|s| !s.is_empty()) {
        Some(s) => Some(Lyrics { synced: true, lines: parse_lrc(s) }),
        None => Some(Lyrics { synced: false, lines: best["plainLyrics"].as_str()?.lines().map(|l| (0.0, l.trim().to_string())).collect() }),
    }
}

/// Blocks on the network: call it from a thread. None when lrclib has nothing for this track.
pub fn fetch(t: &Track) -> Option<Lyrics> {
    queries(t).into_iter().find_map(|q| {
        let out = Command::new("curl").args(["-s", "-m", "15", "-A", "seiza-wonder", "-G", "https://lrclib.net/api/search", "--data-urlencode"]).arg(format!("q={q}")).output().ok()?;
        pick(&String::from_utf8_lossy(&out.stdout), t.secs)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_youtube_title_becomes_a_search() {
        assert_eq!(clean("【Hatsune Miku】 World is Mine / ryo（supercell）【初音ミク】"), "World is Mine ryo");
        assert_eq!(clean("Song (Official Video) [4K]"), "Song");
        assert_eq!(clean("Plain Title"), "Plain Title");
    }

    #[test]
    fn a_second_query_drops_feat_and_adds_the_artist() {
        let t = |title: &str, artist: &str| Track { id: "x".into(), title: title.into(), artist: artist.into(), secs: 0 };
        assert_eq!(queries(&t("ワンダー / r-906 feat. 初音ミク", "r-906")), vec!["ワンダー r-906 feat. 初音ミク", "ワンダー r-906"]);
        assert_eq!(queries(&t("Song (Official Video)", "Band")), vec!["Song", "Song Band"]);
        assert_eq!(queries(&t("Song", "")), vec!["Song"]);
    }

    #[test]
    fn timed_lines_are_read_in_order_with_pauses_kept() {
        let l = parse_lrc("[ar:someone]\n[00:20.50] first\n[00:05.00][01:00.00] twice\n[00:30.00]\n");
        assert_eq!(l, vec![(5.0, "twice".into()), (20.5, "first".into()), (30.0, "".into()), (60.0, "twice".into())]);
    }

    #[test]
    fn the_answer_closest_in_length_with_timed_lines_wins() {
        let json = r#"[
            {"duration":164.0,"syncedLyrics":"[00:01.00] wrong song","plainLyrics":"wrong song"},
            {"duration":253.0,"syncedLyrics":null,"plainLyrics":"plain one\ntwo"},
            {"duration":254.0,"syncedLyrics":"[00:02.00] right","plainLyrics":"right"}]"#;
        let l = pick(json, 253).unwrap();
        assert_eq!((l.synced, l.lines), (true, vec![(2.0, "right".to_string())]));
        let plain = pick(&json.replace("[00:02.00] right", ""), 253).unwrap();
        assert!(!plain.synced, "no timed lines in range: the plain ones");
        assert_eq!(pick(json, 400), None, "nothing about that long");
        assert_eq!(pick("not json", 10), None);
    }

    #[test]
    fn the_current_line_is_the_last_one_that_has_started() {
        let l = Lyrics { synced: true, lines: vec![(5.0, "a".into()), (10.0, "b".into())] };
        assert_eq!((l.current(1.0), l.current(5.0), l.current(9.9), l.current(99.0)), (None, Some(0), Some(1), Some(1)));
    }
}
