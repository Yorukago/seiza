//! The sky's colors. Defaults are the design brief's palette; wallust (or
//! anything else) overrides them through `$XDG_CONFIG_HOME/seiza/palette`,
//! one `role=#rrggbb` per line. Roles: sky dust mist glow star text.

use std::{collections::HashMap, sync::OnceLock};

pub type Rgb = [f32; 3];

pub struct Palette {
    pub sky: Rgb,   // background
    pub panel: Rgb, // bar, menus: the sky lifted a little toward dust
    pub dust: Rgb,  // deep nebula
    pub mist: Rgb,  // mid nebula
    pub glow: Rgb,  // bright nebula edges
    pub star: Rgb,  // star glow, window frame: gold unless the file says otherwise
    pub text: Rgb,
    pub ice: Rgb,   // star cores, work mode
    pub pink: Rgb,  // tiny moments only
}

pub fn rgba(c: Rgb, a: f32) -> [f32; 4] {
    [c[0], c[1], c[2], a]
}

pub fn hex(s: &str) -> Option<Rgb> {
    let v = u32::from_str_radix(s.trim().trim_start_matches('#'), 16).ok()?;
    Some([(v >> 16 & 255) as f32 / 255.0, (v >> 8 & 255) as f32 / 255.0, (v & 255) as f32 / 255.0])
}

fn lum(c: Rgb) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

impl Palette {
    pub fn parse(file: &str) -> Palette {
        let map: HashMap<&str, Rgb> = file
            .lines()
            .filter_map(|l| l.split_once('='))
            .filter_map(|(k, v)| Some((k.trim(), hex(v)?)))
            .collect();
        let get = |k: &str, d: &str| map.get(k).copied().unwrap_or_else(|| hex(d).unwrap());
        let sky = get("sky", "0C0A12");
        let dust = get("dust", "2A2150");
        let mut text = get("text", "F2E8C9");
        if lum(text) - lum(sky) < 0.5 {
            text = hex("F2E8C9").unwrap(); // contrast guard: text must stay readable on the sky
        }
        Palette {
            sky,
            panel: mix(sky, dust, 0.18),
            dust,
            mist: get("mist", "4B3F86"),
            glow: get("glow", "7A6BB8"),
            star: get("star", "E9D48F"),
            text,
            ice: hex("A9D8F5").unwrap(),
            pink: hex("E8A0D0").unwrap(),
        }
    }
}

pub fn palette() -> &'static Palette {
    static P: OnceLock<Palette> = OnceLock::new();
    P.get_or_init(|| {
        let dir = std::env::var("XDG_CONFIG_HOME")
            .unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
        Palette::parse(&std::fs::read_to_string(format!("{dir}/seiza/palette")).unwrap_or_default())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_overrides() {
        assert_eq!(Palette::parse("").star, hex("E9D48F").unwrap());
        let p = Palette::parse("sky=#000000\nstar = ff0000\nbogus\ntext=#101010");
        assert_eq!(p.sky, [0.0; 3]);
        assert_eq!(p.star, [1.0, 0.0, 0.0]);
        assert_eq!(p.text, hex("F2E8C9").unwrap()); // dark text is rejected
    }
}
