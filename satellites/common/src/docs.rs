//! The documentation: what the commands, the words, the config recipes, the options and the files are.
//! Plain data, shown by the settings' "documentation" page (and found by its search).

pub type Cards = &'static [(&'static str, &'static [(&'static str, &'static str)])];

pub const COMMANDS: Cards = &[
    ("sky", &[
        ("seiza overview", "zoom out over every star"),
        ("seiza pan DX DY", "move the camera by that many pixels"),
        ("seiza zoom-reset", "back to full size"),
        ("seiza goto N", "fly to star number N"),
        ("seiza new-star", "make a star next to the current one"),
        ("seiza delete-star [N]", "delete star N (default: current); it must be empty"),
        ("seiza rename-star N NAME", "give star N a name"),
        ("seiza add-star X Y", "a star centered there"),
        ("seiza move-star N X Y", "move a star"),
        ("seiza link A B", "toggle a thread between two stars"),
        ("seiza figure NAME|none", "lay the stars out on a template"),
    ]),
    ("constellations", &[
        ("seiza save NAME / load NAME", "keep or open a constellation"),
        ("seiza new-constellation NAME", "a fresh one with a single star"),
    ]),
    ("windows", &[
        ("seiza move-window DX DY", "move the focused window"),
        ("seiza resize-window DW DH", "grow or shrink it"),
        ("seiza focus DX DY", "focus the nearest window that way"),
        ("seiza maximize / close-window", "the focused window"),
        ("seiza tile", "tile or untile this star (remembered)"),
        ("seiza switch", "the window switcher (Super+Tab)"),
        ("seiza cycle-window", "focus the next window"),
    ]),
    ("session", &[
        ("seiza reload-config", "re-read keys, motion and the frame"),
        ("seiza relaunch", "restart the satellites"),
        ("seiza quit", "leave Seiza"),
        ("anything else", "runs as a shell command"),
    ]),
];

pub const GUIDE: Cards = &[
    ("the sky", &[
        ("star", "a place on the big canvas. Windows live in a star."),
        ("constellation", "a set of stars you can save and come back to."),
        ("overview", "zoom out to see every star; click one to fly in."),
        ("thread", "a line you draw between two stars, just for you."),
    ]),
    ("programs", &[
        ("satellite", "a small program Seiza starts: bar, sky, launcher..."),
        ("launcher", "type to find an app; Tab flips list and stars."),
        ("quick answers", "launcher: 2*(3+4), 5 km to mi, :moon. Enter copies."),
        ("settings", "colors, frame, bar, sky and motion; most apply at once."),
        ("lock", "the lock screen; it asks for your normal password."),
    ]),
    ("windows and cards", &[
        ("tiling", "a tiled star arranges its windows; Super+t flips it."),
        ("rules", "rule in seiza.kdl: send an app to a star, size it."),
        ("cards", "drag to pin a card to a star; right-click moves it on."),
        ("every window", "can be moved and resized from the keyboard too."),
    ]),
];

/// Copy these into ~/.config/seiza/seiza.kdl.
pub const RECIPES: Cards = &[
    ("look", &[
        ("hide the bar", "bar { autohide on }  The bar becomes a thin strip at the top edge and grows when the pointer touches it."),
        ("change the wallpaper by itself", "look { slideshow 30 }  Every 30 minutes the next picture in the wallpaper's folder recolors the sky. 0 is off."),
        ("see the weather", "weather { place \"Lisbon\" }, then turn on the weather card in settings, widgets. Without a place it guesses from your address."),
        ("see what is coming up", "calendar { source \"~/cal.ics\" }, then turn on the agenda card. A calendar's private address works too."),
    ]),
    ("windows", &[
        ("tile every star", "layout { tile on; max 3; gap 10 }  The first 3 windows of a star are laid out for you. Super+t flips one star."),
        ("send an app to a star", "rule app=\"steam\" star=\"games\" mode=\"maximize\"  The first rule that matches wins. seiza debug-windows shows app ids."),
        ("size a window", "rule app=\"pavucontrol\" w=700 h=500  Give both w and h. title=\"part of the title\" matches by title instead."),
    ]),
    ("session", &[
        ("lock when you leave", "idle { dim 120; lock 300; off 600 }  Seconds without input. 0 is never. Anything watching video keeps it awake."),
        ("choose what starts", "autostart { spawn \"seiza-bar\"; spawn \"seiza-starfield\" }  Leave it out to get the usual ones. An empty block starts nothing."),
        ("check your file", "seiza-conf check  names mistakes by line. Saving the file reloads it by itself."),
    ]),
    ("keys", &[
        ("bind a key", "binds { Super+Return { spawn \"kitty\" } }  Several actions: Super+g { goto 1; spawn \"kitty\"; }"),
        ("name a few actions", "def my-thing { goto 1; spawn \"notify-send hi\"; }  then use my-thing in a bind."),
    ]),
];

pub const FILES: Cards = &[
    ("in ~/.config/seiza", &[
        ("seiza.kdl", "most settings in one file; seiza-conf docs lists them."),
        ("keys", "Mods+Key = command. Yours add to the defaults. Saved = shown here."),
        ("motion", "how the camera glides: pan_secs, easing, reduce."),
        ("palette", "the sky's colors (role=#hex). seiza-wallpaper writes it."),
        ("frame", "window corner radius, border and glow."),
        ("sky", "the starfield: nebula and friends."),
        ("look", "clock format, wallpaper, palette style, slideshow minutes."),
        ("widgets", "which cards show; weather and agenda are opt-in."),
        ("launcher", "style, search and terminal."),
        ("autostart", "programs to start with Seiza, one per line."),
    ]),
    ("yours to make", &[
        ("figures/NAME.fig", "your own constellation templates."),
        ("*.const", "saved constellations."),
    ]),
];

/// The options table as cards: one per section, its blurb first, then each option with its range and default.
pub fn options_cards() -> Vec<(String, Vec<(String, String)>)> {
    use crate::conf::{Kind, OPTS, SECTIONS};
    SECTIONS
        .iter()
        .map(|(sec, what)| {
            let mut rows = vec![(String::new(), what.to_string())];
            for o in OPTS.iter().filter(|o| o.section == *sec) {
                let hint = match &o.kind {
                    Kind::Flag => "on or off".to_string(),
                    Kind::Num(lo, hi) => format!("{lo} to {hi}"),
                    Kind::Pick(list) => list.join(", "),
                    Kind::Text => "text".to_string(),
                };
                let default = if o.default.is_empty() { "empty".to_string() } else { o.default.to_string() };
                rows.push((o.key.to_string(), format!("{}  ({hint}; default {default})", o.doc)));
            }
            (sec.to_string(), rows)
        })
        .collect()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_list_every_setting_with_its_default() {
        let cards = options_cards();
        let rows: Vec<&(String, String)> = cards.iter().flat_map(|(_, r)| r).collect();
        for key in ["autohide", "slideshow", "place", "source", "tile"] {
            assert!(rows.iter().any(|(l, _)| l == key), "{key} missing");
        }
        assert!(rows.iter().any(|(l, r)| l == "radius" && r.contains("default 14")));
    }

    #[test]
    fn every_documentation_row_says_something() {
        for set in [COMMANDS, GUIDE, RECIPES, FILES] {
            for (title, rows) in set {
                assert!(!title.is_empty() && !rows.is_empty());
                assert!(rows.iter().all(|(name, what)| !name.is_empty() && !what.is_empty()), "{title}");
            }
        }
    }
}
