//! The notification log seiza-notify writes (one line per toast: unix time, app, summary, body)
//! and the time the panel last showed it, so the bar can tell whether there is something unread.

use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()))
}

/// The log: written by seiza-notify, read by seiza-panel.
pub fn log_path() -> PathBuf {
    dir().join("seiza-notifications")
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The panel showed the log (or cleared it): everything up to now counts as read.
pub fn mark_seen() {
    let _ = std::fs::write(dir().join("seiza-notifications-seen"), now().to_string());
}

/// How many log lines are newer than the last `mark_seen`.
pub fn unread() -> usize {
    let seen: u64 = std::fs::read_to_string(dir().join("seiza-notifications-seen")).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    let log = std::fs::read_to_string(log_path()).unwrap_or_default();
    log.lines().filter(|l| l.split('\t').next().and_then(|t| t.parse::<u64>().ok()).is_some_and(|t| t > seen)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_counts_lines_after_the_seen_time() {
        let dir = std::env::temp_dir().join(format!("seiza-notes-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: the only test in the crate that sets XDG_RUNTIME_DIR.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &dir) };
        assert_eq!(unread(), 0, "no log is nothing unread");
        std::fs::write(log_path(), "10\ta\tone\t\n20\ta\ttwo\t\nnot a line\n").unwrap();
        assert_eq!(unread(), 2, "never seen: everything is new");
        std::fs::write(dir.join("seiza-notifications-seen"), "10").unwrap();
        assert_eq!(unread(), 1, "only the line after the seen time");
        mark_seen();
        assert_eq!(unread(), 0, "marked seen now");
        let _ = std::fs::remove_dir_all(dir);
    }
}
