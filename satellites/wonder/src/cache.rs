//! The cache under `~/.cache/seiza/wonder`: covers, and the audio of the track that plays next, so it starts at once.
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

/// The audio folder is trimmed to this many bytes, oldest files first: `wonder/cache_gb` in the settings (2 GB).
fn audio_limit() -> u64 {
    let gb = seiza_common::config::get("wonder", "cache_gb").and_then(|v| v.parse::<f64>().ok()).unwrap_or(2.0);
    (gb.clamp(1.0, 100.0) * (1u64 << 30) as f64) as u64
}

pub fn root() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join(".cache")))?;
    Some(base.join("seiza/wonder"))
}

fn audio_dir() -> Option<PathBuf> {
    root().map(|r| r.join("audio"))
}

/// A YouTube id becomes part of a file name, so only the characters ids have.
pub fn valid(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The finished download of this track, if there is one: `id.webm`, never `id.webm.part` (its stem is `id.webm`).
pub fn audio(id: &str) -> Option<PathBuf> {
    if !valid(id) {
        return None;
    }
    std::fs::read_dir(audio_dir()?).ok()?.filter_map(Result::ok).map(|e| e.path()).find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(id))
}

/// Download the audio of a track unless it is already here, then trim the folder. Blocks: call it from a thread.
pub fn fetch(id: &str) {
    let Some(dir) = audio_dir().filter(|_| valid(id) && audio(id).is_none()) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let ok = Command::new("yt-dlp")
        .args(["-q", "--no-warnings", "--no-playlist", "-f", "bestaudio", "-o"])
        .arg(dir.join(format!("{id}.%(ext)s")))
        .arg(format!("https://www.youtube.com/watch?v={id}"))
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        let files = std::fs::read_dir(&dir).into_iter().flatten().filter_map(Result::ok).filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        });
        evict(files.collect(), audio_limit()).into_iter().for_each(|p| drop(std::fs::remove_file(p)));
    }
}

/// The files to delete so the rest fit in `limit` bytes: the oldest go first.
fn evict(mut files: Vec<(SystemTime, u64, PathBuf)>, limit: u64) -> Vec<PathBuf> {
    files.sort_by_key(|f| f.0);
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let mut out = vec![];
    for (_, size, path) in files {
        if total <= limit {
            break;
        }
        total -= size;
        out.push(path);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn the_oldest_files_go_until_the_rest_fit() {
        let f = |age: u64, size, name: &str| (SystemTime::UNIX_EPOCH + Duration::from_secs(age), size, PathBuf::from(name));
        let files = vec![f(30, 40, "new"), f(10, 40, "old"), f(20, 40, "mid")];
        assert_eq!(evict(files.clone(), 120), Vec::<PathBuf>::new(), "exactly at the limit stays");
        assert_eq!(evict(files.clone(), 100), vec![PathBuf::from("old")]);
        assert_eq!(evict(files, 50), vec![PathBuf::from("old"), PathBuf::from("mid")]);
    }

    #[test]
    fn only_id_characters_reach_a_file_name() {
        assert!(valid("dQw4w9WgXcQ") && valid("a-b_c"));
        assert!(!valid("") && !valid("../x") && !valid("a.b"));
    }
}
