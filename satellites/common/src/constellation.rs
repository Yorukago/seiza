//! Seiza's constellation files (`~/.config/seiza/<name>.const`), read-only: enough for a
//! program to draw them. The compositor writes them; the format is in src/constellation.h.

use crate::config;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Constellation {
    pub name: String,
    pub stars: Vec<(String, f64, f64)>, // name, x, y (the middle of the star's territory)
    pub links: Vec<(usize, usize)>,
}

/// None for anything that is not a whole constellation file (no header, no `end`, a bad line).
pub fn parse(text: &str) -> Option<Constellation> {
    let mut c = Constellation::default();
    let (mut header, mut end) = (false, false);
    for line in text.lines() {
        if end {
            return None;
        } else if line.is_empty() || line.starts_with('#') || line.starts_with("app ") || line.starts_with("point ") || line.starts_with("figure ") {
            continue; // remembered apps and what older files said about figures are not ours to draw
        } else if let Some(name) = line.strip_prefix("constellation ").filter(|_| !header) {
            c.name = name.to_string();
            header = true;
        } else if let Some(rest) = line.strip_prefix("star ").filter(|_| header) {
            let mut w = rest.splitn(3, ' ');
            let (x, y, name) = (w.next()?.parse().ok()?, w.next()?.parse().ok()?, w.next()?);
            c.stars.push((name.to_string(), x, y));
        } else if let Some(rest) = line.strip_prefix("link ").filter(|_| header) {
            let mut w = rest.split_whitespace();
            let (a, b): (usize, usize) = (w.next()?.parse().ok()?, w.next()?.parse().ok()?);
            c.links.push((a, b));
        } else if line == "end" && header {
            end = true;
        } else {
            return None;
        }
    }
    let n = c.stars.len();
    c.links.retain(|&(a, b)| a != b && a < n && b < n);
    (header && end && n > 0).then_some(c)
}

/// The names of the constellations there are, A-Z.
pub fn list() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(config::path("")).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str()?.strip_suffix(".const").map(String::from)).collect();
    names.sort();
    names
}

pub fn load(name: &str) -> Option<Constellation> {
    if !crate::figure::name_ok(name) {
        return None;
    }
    parse(&std::fs::read_to_string(config::path(&format!("{name}.const"))).ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_compositor_writes() {
        let c = parse("constellation work\nstar 960 540 star 0\napp kitty\nstar 3360.5 540 north star\nlink 0 1\nlink 1 9\nend\n").unwrap();
        assert_eq!(c.name, "work");
        assert_eq!(c.stars, [("star 0".to_string(), 960.0, 540.0), ("north star".to_string(), 3360.5, 540.0)]);
        assert_eq!(c.links, [(0, 1)], "a thread to a star that is not there is dropped");
    }

    #[test]
    fn older_files_and_bad_ones() {
        assert!(parse("constellation x\nstar 1 2 a\npoint 1\nfigure w\nend\n").is_some(), "older files are still read");
        for bad in ["", "constellation x\nstar 1 2 a\n", "star 1 2 a\nend\n", "constellation x\nend\n", "constellation x\nstar 1 a\nend\n", "constellation x\nstar 1 2 a\nend\nstar 3 4 b\n"] {
            assert!(parse(bad).is_none(), "{bad:?}");
        }
    }
}
