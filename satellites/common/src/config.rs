//! Seiza's small config files: `~/.config/seiza/<file>`, lines of `key=value`,
//! `#` comments. The settings page writes them; each program reads its own.

use std::path::PathBuf;

pub fn path(file: &str) -> PathBuf {
    let dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(dir).join("seiza").join(file)
}

fn value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().filter(|l| !l.trim_start().starts_with('#')).filter_map(|l| l.split_once('=')).find(|(k, _)| k.trim() == key).map(|(_, v)| v.trim())
}

pub fn get(file: &str, key: &str) -> Option<String> {
    value(&std::fs::read_to_string(path(file)).ok()?, key).map(String::from)
}

/// `text` with `key` set to `val`: replaces the line if there is one, else adds it. Other lines stay as they are.
fn with_value(text: &str, key: &str, val: &str) -> String {
    let mut done = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| match l.split_once('=') {
            Some((k, _)) if !l.trim_start().starts_with('#') && k.trim() == key => {
                done = true;
                format!("{key}={val}")
            }
            _ => l.to_string(),
        })
        .collect();
    if !done {
        out.push(format!("{key}={val}"));
    }
    out.join("\n") + "\n"
}

pub fn set(file: &str, key: &str, val: &str) -> std::io::Result<()> {
    let p = path(file);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, with_value(&std::fs::read_to_string(&p).unwrap_or_default(), key, val))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_rewrites_one_key_and_keeps_the_rest() {
        let t = "# sky\nnebula = 0.4\nother=1\n#nebula=9\n";
        assert_eq!(value(t, "nebula"), Some("0.4"));
        assert_eq!(value(t, "missing"), None);
        assert_eq!(with_value(t, "nebula", "0.7"), "# sky\nnebula=0.7\nother=1\n#nebula=9\n");
        assert_eq!(with_value("", "a", "b"), "a=b\n");
        assert_eq!(with_value("x=1", "a", "b"), "x=1\na=b\n");
    }
}
