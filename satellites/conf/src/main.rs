//! `seiza-conf check | docs | export <part>`: the tool side of ~/.config/seiza/seiza.kdl.
use seiza_common::{conf, config};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["docs"] => print!("{}", conf::template()),
        ["check"] => match conf::load() {
            None => {
                eprintln!("no {}; start one with: seiza-conf docs > {}", config::path("seiza.kdl").display(), config::path("seiza.kdl").display());
                std::process::exit(1);
            }
            Some(c) if c.errors.is_empty() => println!("seiza.kdl ok: {} option(s), {} bind(s)", c.values.len(), c.binds.len()),
            Some(c) => {
                c.errors.iter().for_each(|e| eprintln!("{e}"));
                std::process::exit(1);
            }
        },
        // after a (re)load: tell the desktop about mistakes. Needs notify-send and a notification daemon (mako, dunst...)
        ["notify"] => {
            let errors = conf::load().map(|c| c.errors).unwrap_or_default();
            if !errors.is_empty() {
                let body = errors.iter().take(5).cloned().collect::<Vec<_>>().join("\n") + if errors.len() > 5 { "\n..." } else { "" };
                let _ = std::process::Command::new("notify-send").args(["-a", "seiza", "-u", "critical", &format!("seiza.kdl has {} mistake(s)", errors.len()), &body]).status();
            }
        }
        // run by the session: every `look.slideshow` minutes, the next picture of the wallpaper's folder recolors the sky
        ["slideshow"] => slideshow(),
        // for the compositor: one part of the file in the old flat format; mistakes go to stderr (its log)
        ["export", part] => {
            if let Some(c) = conf::load() {
                // the compositor asks for every part; say the mistakes once, with the part it always loads
                if *part == "keys" {
                    c.errors.iter().for_each(|e| eprintln!("seiza: {e}"));
                }
                print!("{}", conf::export(&c, part));
            }
        }
        _ => {
            eprintln!("usage: seiza-conf check | docs | notify | slideshow | export <keys|autostart|motion|frame|...>");
            std::process::exit(2);
        }
    }
}

const PICTURES: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

/// The picture after `current` in its folder (sorted by name, wrapping round); the first one if `current` is not there.
/// With no current picture, the folder is ~/Pictures. None if the folder has no pictures.
fn next_picture(current: &str) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let current = current.trim().replace('~', &home);
    let dir = if current.is_empty() { format!("{home}/Pictures") } else { std::path::Path::new(&current).parent()?.to_string_lossy().into_owned() };
    let mut pics: Vec<String> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|f| f.extension().and_then(|x| x.to_str()).is_some_and(|x| PICTURES.contains(&x.to_lowercase().as_str())))
        .map(|f| f.to_string_lossy().into_owned())
        .collect();
    pics.sort();
    let at = pics.iter().position(|p| *p == current).map_or(0, |i| i + 1);
    pics.get(at % pics.len().max(1)).cloned()
}

fn slideshow() {
    let mut waited = 0u64;
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let mins = config::get("look", "slideshow").and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(0);
        waited += 1;
        if mins == 0 || waited < mins {
            continue;
        }
        waited = 0;
        let Some(next) = next_picture(&config::get("look", "wallpaper").unwrap_or_default()) else { continue };
        let palette = match config::get("look", "style").as_deref() {
            Some("hard") => "harddark",
            Some("comp") => "darkcomp",
            Some("salient") => "saliencedark",
            _ => "dark",
        };
        // the same steps as the settings page: recolor, remember the picture, restart what reads the palette at start
        if std::process::Command::new("seiza-wallpaper").args(["-p", palette, &next]).status().is_ok_and(|s| s.success()) {
            let _ = config::set("look", "wallpaper", &next);
            for name in ["seiza-starfield", "seiza-bar", "seiza-widgets", "seiza-osd"] {
                let _ = std::process::Command::new("pkill").args(["-x", name]).status();
                let _ = seiza_common::command(name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_picture_wraps_round_its_folder() {
        let dir = std::env::temp_dir().join(format!("seiza-slideshow-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["a.png", "b.JPG", "c.txt", "d.webp"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let p = |f: &str| dir.join(f).to_string_lossy().into_owned();
        assert_eq!(next_picture(&p("a.png")).unwrap(), p("b.JPG"));
        assert_eq!(next_picture(&p("d.webp")).unwrap(), p("a.png"), "wraps");
        assert_eq!(next_picture(&p("gone.png")).unwrap(), p("a.png"), "a picture that is not there starts over");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
