//! What was played, newest first: one file of tracks (the playlist format, named "history") under the data folder.
//! A track is logged once it has played a few seconds, and again only moves to the top; the oldest fall off at `MAX`.
use crate::search::Track;
use std::path::PathBuf;

const MAX: usize = 200;

fn file() -> Option<PathBuf> {
    crate::cache::data_dir().map(|d| d.join("history.txt"))
}

/// The log, oldest first, as stored.
fn read() -> Vec<Track> {
    file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| crate::playlists::parse(&t)).map_or(vec![], |(_, tracks)| tracks)
}

fn write(tracks: &[Track]) {
    let Some(f) = file() else { return };
    let _ = f.parent().map(std::fs::create_dir_all);
    let _ = std::fs::write(f, crate::playlists::render("history", tracks));
}

/// What was played, the newest first.
pub fn list() -> Vec<Track> {
    let mut all = read();
    all.reverse();
    all
}

/// `all` with `t` as the newest entry: any earlier play of it is dropped, and only the last `MAX` stay.
fn with_played(mut all: Vec<Track>, t: &Track) -> Vec<Track> {
    all.retain(|x| x.id != t.id);
    all.push(t.clone());
    let extra = all.len().saturating_sub(MAX);
    all.drain(..extra);
    all
}

pub fn record(t: &Track) {
    write(&with_played(read(), t));
}

pub fn remove(id: &str) {
    let mut all = read();
    all.retain(|x| x.id != id);
    write(&all);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str) -> Track {
        Track { id: id.into(), title: id.into(), artist: String::new(), secs: 1 }
    }

    #[test]
    fn a_replayed_track_moves_to_the_top_and_the_oldest_fall_off() {
        let ids = |v: &[Track]| v.iter().map(|t| t.id.clone()).collect::<Vec<_>>().join(",");
        let all = vec![t("a"), t("b"), t("c")];
        assert_eq!(ids(&with_played(all.clone(), &t("a"))), "b,c,a");
        assert_eq!(ids(&with_played(all, &t("d"))), "a,b,c,d");
        let many: Vec<Track> = (0..MAX).map(|i| t(&format!("t{i}"))).collect();
        let out = with_played(many, &t("new"));
        assert_eq!((out.len(), out[0].id.as_str(), out[MAX - 1].id.as_str()), (MAX, "t1", "new"));
    }
}
