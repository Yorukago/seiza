//! Constellation figures: where a constellation's stars start (points) and which are
//! joined by threads (links). The same plain-text files the compositor reads:
//! `point u v` and `link a b` lines in `~/.config/seiza/figures/<name>.fig`, over the
//! built-in ones in the repo's `figures/` directory.

use std::path::PathBuf;

const BUILT_IN: [(&str, &str); 4] = [
    ("crown", include_str!("../../../figures/crown.fig")),
    ("dipper", include_str!("../../../figures/dipper.fig")),
    ("kite", include_str!("../../../figures/kite.fig")),
    ("w", include_str!("../../../figures/w.fig")),
];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Figure {
    pub points: Vec<(f32, f32)>,
    pub links: Vec<(usize, usize)>,
}

/// Names are file names: letters, digits, `_` and `-`, not starting with `-`, up to 31.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 31 && !name.starts_with('-') && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Figure {
    /// None if the text is not a figure: no points, a bad line, or a link to a point that is not there.
    pub fn parse(text: &str) -> Option<Figure> {
        let mut f = Figure::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let mut w = line.split_whitespace();
            match (w.next()?, w.next(), w.next(), w.next()) {
                ("point", Some(u), Some(v), None) => f.points.push((u.parse().ok().filter(|x: &f32| x.is_finite())?, v.parse().ok().filter(|x: &f32| x.is_finite())?)),
                ("link", Some(a), Some(b), None) => f.links.push((a.parse().ok()?, b.parse().ok()?)),
                _ => return None,
            }
        }
        let n = f.points.len();
        (n > 0 && f.links.iter().all(|&(a, b)| a != b && a < n && b < n)).then_some(f)
    }

    pub fn format(&self) -> String {
        let mut s: String = self.points.iter().map(|(u, v)| format!("point {u:.3} {v:.3}\n")).collect();
        s.extend(self.links.iter().map(|(a, b)| format!("link {a} {b}\n")));
        s
    }
}

fn dir() -> PathBuf {
    crate::config::path("figures")
}

pub fn is_built_in(name: &str) -> bool {
    BUILT_IN.iter().any(|(n, _)| *n == name)
}

/// Built-in figures first, then the ones in the user's directory, A-Z within each.
pub fn list() -> Vec<String> {
    let mut mine: Vec<String> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".fig").map(String::from))
        .filter(|n| name_ok(n) && !is_built_in(n))
        .collect();
    mine.sort();
    BUILT_IN.iter().map(|(n, _)| n.to_string()).chain(mine).collect()
}

/// The user's file of that name wins over a built-in one.
pub fn load(name: &str) -> Option<Figure> {
    if !name_ok(name) {
        return None;
    }
    std::fs::read_to_string(dir().join(format!("{name}.fig"))).ok().and_then(|t| Figure::parse(&t)).or_else(|| BUILT_IN.iter().find(|(n, _)| *n == name).and_then(|(_, t)| Figure::parse(t)))
}

pub fn save(name: &str, f: &Figure) -> std::io::Result<()> {
    if !name_ok(name) || f.points.is_empty() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "a name of letters, digits, _ and -, and at least one point"));
    }
    std::fs::create_dir_all(dir())?;
    std::fs::write(dir().join(format!("{name}.fig")), f.format())
}

/// Removes the user's file of that name (a built-in of the same name comes back).
pub fn delete(name: &str) -> std::io::Result<()> {
    if !name_ok(name) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad name"));
    }
    std::fs::remove_file(dir().join(format!("{name}.fig")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_the_compositor_parses() {
        let f = Figure::parse("# two\npoint 0 0\npoint 1.5 0.25\nlink 0 1\n").unwrap();
        assert_eq!((f.points.len(), f.links.clone()), (2, vec![(0, 1)]));
        assert_eq!(Figure::parse(&f.format()), Some(f));
        for bad in ["", "point 0 0\nlink 0 1\n", "point 0 0\npoint 1 1\nlink 1 1\n", "point 0\n", "point 0 0\nwat\n", "point 0 0 0\n", "point nan 0\n"] {
            assert!(Figure::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn the_built_in_figures_are_valid_and_listed_first() {
        for (n, _) in BUILT_IN {
            assert!(load(n).is_some_and(|f| f.points.len() >= 4), "{n}");
        }
        assert_eq!(&list()[..4], ["crown", "dipper", "kite", "w"]);
    }

    #[test]
    fn names_are_file_names() {
        assert!(name_ok("my-sky_2") && !name_ok("../x") && !name_ok("-x") && !name_ok("") && !name_ok(&"a".repeat(32)));
        assert!(load("../etc/passwd").is_none());
        assert!(save("a/b", &Figure { points: vec![(0.0, 0.0)], links: vec![] }).is_err());
    }
}
