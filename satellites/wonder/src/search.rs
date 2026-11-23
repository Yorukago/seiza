//! YouTube search through yt-dlp: one call, a flat list of what it found.
use std::process::Command;

#[derive(Debug, PartialEq, Clone)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub secs: u64, // 0 when yt-dlp does not know (a live stream)
}

impl Track {
    pub fn url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.id)
    }
}

/// One line per result, fields split by U+001F, like the widgets' playerctl format.
const FORMAT: &str = "%(id)s\u{1f}%(title)s\u{1f}%(channel)s\u{1f}%(duration)s";

/// Blocks for a second or two: call it from a thread.
pub fn search(query: &str, count: usize) -> Result<Vec<Track>, String> {
    let out = Command::new("yt-dlp")
        .args(["--flat-playlist", "--no-warnings", "--print", FORMAT])
        .arg(format!("ytsearch{count}:{query}"))
        .output()
        .map_err(|e| format!("yt-dlp did not start: {e}"))?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("yt-dlp failed").to_string());
    }
    Ok(parse(&String::from_utf8_lossy(&out.stdout)))
}

fn parse(out: &str) -> Vec<Track> {
    out.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\u{1f}').collect();
            let known = |s: &str| if s == "NA" { String::new() } else { s.to_string() };
            (f.len() == 4 && !f[0].is_empty()).then(|| Track { id: f[0].into(), title: f[1].into(), artist: known(f[2]), secs: f[3].parse::<f64>().unwrap_or(0.0) as u64 })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_are_read_and_unknowns_become_empty() {
        let out = "abc\u{1f}Song\u{1f}Band\u{1f}226.0\nxyz\u{1f}Live\u{1f}NA\u{1f}NA\nbroken line\n";
        let t = parse(out);
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].title.as_str(), t[0].artist.as_str(), t[0].secs), ("Song", "Band", 226));
        assert_eq!((t[1].artist.as_str(), t[1].secs), ("", 0));
        assert_eq!(t[0].url(), "https://www.youtube.com/watch?v=abc");
    }
}
