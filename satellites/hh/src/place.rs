//! Which of a palette's five colors becomes which Seiza role, and the palette file itself.
//! `star` is not touched, so the stars stay gold (or whatever the file already says).

use crate::catalog::C;
use seiza_common::palette::Palette;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Roles {
    pub sky: C,
    pub dust: C,
    pub mist: C,
    pub glow: C,
    pub text: C,
}

fn luma(c: C) -> f32 {
    0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32
}

fn chroma(c: C) -> u8 {
    c.iter().max().unwrap() - c.iter().min().unwrap()
}

/// Darkest is the sky, lightest the text, the most saturated of the three between is the glow,
/// the other two are dust (darker) and mist.
pub fn place(cs: &[C; 5]) -> Roles {
    let mut s = *cs;
    s.sort_by(|a, b| luma(*a).total_cmp(&luma(*b)));
    let g = (1..4).max_by_key(|&i| (chroma(s[i]), std::cmp::Reverse(i))).unwrap();
    let mut rest = (1..4).filter(|&i| i != g);
    let (dust, mist) = (s[rest.next().unwrap()], s[rest.next().unwrap()]);
    Roles { sky: s[0], dust, mist, glow: s[g], text: s[4] }
}

pub fn hex(c: C) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

impl Roles {
    pub fn list(&self) -> [(&'static str, C); 5] {
        [("sky", self.sky), ("dust", self.dust), ("mist", self.mist), ("glow", self.glow), ("text", self.text)]
    }

    /// The palette file for these roles, keeping a `star=` line from the old file if it has one.
    pub fn file(&self, old: &str) -> String {
        let mut s: String = self.list().iter().map(|(k, c)| format!("{k}={}\n", hex(*c))).collect();
        if let Some(l) = old.lines().find(|l| l.split_once('=').is_some_and(|(k, _)| k.trim() == "star")) {
            s += &format!("{}\n", l.trim());
        }
        s
    }

    /// What a palette file says (defaults for what it leaves out), without the contrast guard.
    pub fn parse(file: &str) -> Roles {
        let p = Palette::parse(file, false);
        let u = |c: [f32; 3]| c.map(|v| (v * 255.0).round() as u8);
        Roles { sky: u(p.sky), dust: u(p.dust), mist: u(p.mist), glow: u(p.glow), text: u(p.text) }
    }

    /// Seiza is made for a dark sky; a palette whose darkest color is this light will look washed out.
    pub fn bright_sky(&self) -> bool {
        luma(self.sky) > 100.0
    }

    pub fn colors(&self) -> [C; 5] {
        self.list().map(|(_, c)| c)
    }
}

/// Writes the palette file (the old one is kept as palette.bak) and restarts what reads it at start.
/// SEIZA_HH_DRY=1 only says what it would do.
pub fn apply(r: &Roles) -> std::io::Result<()> {
    let path = seiza_common::config::path("palette");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if !old.is_empty() {
        std::fs::write(path.with_extension("bak"), &old)?;
    }
    std::fs::write(&path, r.file(&old))?;
    if std::env::var_os("SEIZA_HH_DRY").is_some() {
        println!("wrote {}", path.display());
        return Ok(());
    }
    // Seiza itself hot-reloads the file; the sky and friends read it at start, so they come back with it.
    for name in ["seiza-starfield", "seiza-bar", "seiza-widgets", "seiza-osd"] {
        let _ = std::process::Command::new("pkill").args(["-x", name]).status();
        let _ = seiza_common::command(name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcha_latte_is_placed_by_light_and_saturation() {
        let r = place(&[[40, 54, 24], [138, 154, 91], [168, 184, 121], [245, 245, 220], [139, 90, 43]]);
        assert_eq!(r, Roles { sky: [40, 54, 24], dust: [138, 154, 91], mist: [168, 184, 121], glow: [139, 90, 43], text: [245, 245, 220] });
    }

    #[test]
    fn the_file_keeps_the_star_and_round_trips() {
        let r = place(&[[10, 10, 20], [60, 30, 120], [90, 70, 160], [200, 80, 200], [240, 235, 220]]);
        let f = r.file("sky=#000000\nstar = #ff0000\n");
        assert!(f.ends_with("star = #ff0000\n") || f.ends_with("star = #ff0000"), "{f}");
        assert_eq!(Roles::parse(&f), r);
        assert!(!r.file("sky=#000000\n").contains("star"));
    }

    #[test]
    fn applying_writes_the_file_and_keeps_a_backup() {
        let d = std::env::temp_dir().join(format!("seiza-hh-test-{}", std::process::id()));
        std::fs::create_dir_all(d.join("seiza")).unwrap();
        std::fs::write(d.join("seiza/palette"), "sky=#010203\nstar=#ff0000\n").unwrap();
        // shortcut: env is process-wide, fine while this is the only test that reads it.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &d);
            std::env::set_var("SEIZA_HH_DRY", "1");
        }
        let r = place(&[[10, 10, 20], [60, 30, 120], [90, 70, 160], [200, 80, 200], [240, 235, 220]]);
        apply(&r).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("seiza/palette.bak")).unwrap(), "sky=#010203\nstar=#ff0000\n");
        let now = std::fs::read_to_string(d.join("seiza/palette")).unwrap();
        assert_eq!(Roles::parse(&now), r);
        assert!(now.contains("star=#ff0000"));
        std::fs::remove_dir_all(d).unwrap();
    }
}
