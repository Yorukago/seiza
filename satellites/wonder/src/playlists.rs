//! Saved queues: one text file each under `~/.local/share/seiza/wonder/playlists`, the name on the first line and
//! then one tab-separated line per track (id, title, artist, seconds). Readable, and editable by hand.
use crate::search::Track;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Track>,
    pub file: PathBuf,
}

impl Playlist {
    pub fn secs(&self) -> u64 {
        self.tracks.iter().map(|t| t.secs).sum()
    }
}

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".local/share")))?;
    Some(base.join("seiza/wonder/playlists"))
}

/// A playlist is named after its first song, and how many more follow.
pub fn name_of(tracks: &[Track]) -> String {
    match tracks {
        [] => String::new(),
        [only] => only.title.clone(),
        [first, rest @ ..] => format!("{} +{}", first.title, rest.len()),
    }
}

fn one_line(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn render(name: &str, tracks: &[Track]) -> String {
    let mut out = one_line(name) + "\n";
    for t in tracks {
        out += &format!("{}\t{}\t{}\t{}\n", t.id, one_line(&t.title), one_line(&t.artist), t.secs);
    }
    out
}

/// None for a file with no name or no playable track. A bad line is skipped: its id would end up in a URL and a file name.
fn parse(text: &str) -> Option<(String, Vec<Track>)> {
    let mut lines = text.lines();
    let name = lines.next()?.trim().to_string();
    let tracks: Vec<Track> = lines
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() == 4 && crate::cache::valid(f[0])).then(|| Track { id: f[0].into(), title: f[1].into(), artist: f[2].into(), secs: f[3].parse().unwrap_or(0) })
        })
        .collect();
    (!name.is_empty() && !tracks.is_empty()).then_some((name, tracks))
}

/// Save a queue as a new playlist.
pub fn save(tracks: &[Track]) -> Option<Playlist> {
    if tracks.is_empty() {
        return None;
    }
    let dir = dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let name = name_of(tracks);
    // the file is named by the time; a second save in the same second takes the next free name
    let file = (0..100).map(|n| dir.join(format!("{}.txt", stamp + n))).find(|f| !f.exists())?;
    std::fs::write(&file, render(&name, tracks)).ok()?;
    Some(Playlist { name, tracks: tracks.to_vec(), file })
}

/// Every saved playlist, the newest first.
pub fn list() -> Vec<Playlist> {
    let Some(entries) = dir().and_then(|d| std::fs::read_dir(d).ok()) else { return vec![] };
    let mut all: Vec<Playlist> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .filter_map(|file| {
            let (name, tracks) = parse(&std::fs::read_to_string(&file).ok()?)?;
            Some(Playlist { name, tracks, file })
        })
        .collect();
    all.sort_by(|a, b| b.file.cmp(&a.file));
    all
}

pub fn delete(p: &Playlist) {
    let _ = std::fs::remove_file(&p.file);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, title: &str) -> Track {
        Track { id: id.into(), title: title.into(), artist: "someone".into(), secs: 100 }
    }

    #[test]
    fn a_queue_survives_the_file_and_a_tab_in_a_title_does_not_break_it() {
        let tracks = vec![t("abc_123", "first\tsong"), t("x-y", "second")];
        let (name, back) = parse(&render(&name_of(&tracks), &tracks)).unwrap();
        assert_eq!(name, "first song +1");
        assert_eq!(back.len(), 2);
        assert_eq!((back[0].id.as_str(), back[0].title.as_str(), back[1].secs), ("abc_123", "first song", 100));
    }

    #[test]
    fn a_line_with_a_bad_id_is_skipped_and_an_empty_file_is_nothing() {
        let text = "mine\nok_id\tA\tB\t5\n../evil\tC\tD\t6\nshort line\n";
        let (_, tracks) = parse(text).unwrap();
        assert_eq!(tracks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["ok_id"]);
        assert!(parse("only a name\n").is_none() && parse("").is_none());
    }

    #[test]
    fn a_playlist_is_named_after_its_first_song() {
        assert_eq!(name_of(&[t("a", "Solo")]), "Solo");
        assert_eq!(name_of(&[t("a", "One"), t("b", "Two"), t("c", "Three")]), "One +2");
    }
}
