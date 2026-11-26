//! HueHaven's palettes: the built-in table (its own palettes.c, compiled by build.rs), then your
//! ~/.config/huehaven/palettes.conf and favorites, read the way HueHaven's README documents them.

use std::path::PathBuf;

pub type C = [u8; 3];

pub struct Pal {
    pub name: String,
    pub mood: String,
    pub collection: String,
    pub colors: [C; 5],
    pub fav: bool,
}

#[cfg(hh)]
mod ffi {
    use std::os::raw::{c_char, c_int};
    #[repr(C)]
    pub struct Rgb(pub u8, pub u8, pub u8);
    #[repr(C)]
    pub struct Palette {
        pub kind: c_int, // 0 = PAL_RGB, 1 = PAL_ANSI
        pub name: *const c_char,
        pub mood: *const c_char,
        pub rgb: [Rgb; 5],
        pub ansi_codes: [*const c_char; 5],
        pub ansi_names: [*const c_char; 5],
    }
    #[repr(C)]
    pub struct Collection {
        pub name: *const c_char,
        pub palettes: *const Palette,
        pub num: c_int,
    }
    unsafe extern "C" {
        pub static collections: [Collection; 0];
        pub static num_collections: c_int;
    }
}

#[cfg(hh)]
fn builtin() -> Vec<Pal> {
    use std::ffi::CStr;
    let s = |p: *const std::os::raw::c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    let mut out = vec![];
    unsafe {
        for i in 0..ffi::num_collections as usize {
            let c = &*ffi::collections.as_ptr().add(i);
            for j in 0..c.num as usize {
                let p = &*c.palettes.add(j);
                if p.kind == 0 {
                    // ANSI palettes are escape codes, not colors
                    out.push(Pal { name: s(p.name), mood: s(p.mood), collection: s(c.name), colors: p.rgb.each_ref().map(|c| [c.0, c.1, c.2]), fav: false });
                }
            }
        }
    }
    out
}

#[cfg(not(hh))]
fn builtin() -> Vec<Pal> {
    vec![]
}

pub fn hh_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(base).join("huehaven")
}

fn hex(s: &str) -> Option<C> {
    let v = u32::from_str_radix(s.trim().trim_start_matches('#'), 16).ok().filter(|_| s.trim().trim_start_matches('#').len() == 6)?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// `name|mood|RRGGBB,RRGGBB,RRGGBB,RRGGBB,RRGGBB` a line; blank lines, `#` lines and anything malformed are skipped.
pub fn parse_conf(text: &str) -> Vec<Pal> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|l| {
            let mut f = l.splitn(3, '|');
            let (name, mood, cols) = (f.next()?.trim(), f.next()?.trim(), f.next()?);
            let cols: Vec<C> = cols.split(',').map(hex).collect::<Option<_>>()?;
            Some(Pal { name: name.into(), mood: mood.into(), collection: "Custom".into(), colors: cols.try_into().ok()?, fav: false })
        })
        .collect()
}

/// Everything, your Custom palettes first.
pub fn load() -> Vec<Pal> {
    let read = |f: &str| std::fs::read_to_string(hh_dir().join(f)).unwrap_or_default();
    let favs = read("favorites");
    let mut all = parse_conf(&read("palettes.conf"));
    all.extend(builtin());
    for p in &mut all {
        p.fav = favs.lines().any(|l| l.trim() == p.name);
    }
    all
}

/// Adds one palette to HueHaven's Custom collection.
pub fn append(name: &str, mood: &str, colors: &[C; 5]) -> std::io::Result<()> {
    use std::io::Write;
    std::fs::create_dir_all(hh_dir())?;
    let cols: Vec<String> = colors.iter().map(|c| format!("{:02x}{:02x}{:02x}", c[0], c[1], c[2])).collect();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(hh_dir().join("palettes.conf"))?;
    writeln!(f, "{name}|{mood}|{}", cols.join(","))
}

/// Adds the palette to HueHaven's favorites file, or takes it out if it is there; true if it is a favorite now.
pub fn toggle_fav(name: &str) -> std::io::Result<bool> {
    toggle_in(&hh_dir().join("favorites"), name)
}

fn toggle_in(path: &std::path::Path, name: &str) -> std::io::Result<bool> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let mut names: Vec<&str> = old.lines().filter(|l| !l.trim().is_empty()).collect();
    let was = names.iter().any(|n| n.trim() == name);
    if was {
        names.retain(|n| n.trim() != name);
    } else {
        names.push(name);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, names.iter().map(|n| format!("{}\n", n.trim())).collect::<String>())?;
    Ok(!was)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conf_lines() {
        let p = parse_conf("# mine\n\nTerminal Dusk|Muted purples|1a1b26,414868,#7aa2f7,bb9af7,c0caf5\nshort|x|111111,222222\nbad|x|zz1111,222222,333333,444444,555555\n");
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].name.as_str(), p[0].collection.as_str(), p[0].colors[2]), ("Terminal Dusk", "Custom", [0x7a, 0xa2, 0xf7]));
    }

    #[test]
    fn favorites_toggle_in_hueheavens_file() {
        let f = std::env::temp_dir().join(format!("seiza-hh-fav-{}/favorites", std::process::id()));
        assert!(toggle_in(&f, "Matcha Latte").unwrap());
        assert!(toggle_in(&f, "Iced Taro").unwrap());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "Matcha Latte\nIced Taro\n");
        assert!(!toggle_in(&f, "Matcha Latte").unwrap());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "Iced Taro\n");
        std::fs::remove_dir_all(f.parent().unwrap()).unwrap();
    }

    #[cfg(hh)]
    #[test]
    fn built_ins_come_from_huehaven() {
        let b = builtin();
        let m = b.iter().find(|p| p.name == "Matcha Latte").expect("Matcha Latte");
        assert_eq!((m.collection.as_str(), m.colors[0], m.colors[4]), ("Café & Bakery", [40, 54, 24], [139, 90, 43]));
        assert!(b.len() > 60);
    }
}
