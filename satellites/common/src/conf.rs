//! `~/.config/seiza/seiza.kdl`: every setting in one KDL file, checked against
//! the table below. The same table writes the docs (`seiza-conf docs`), so an
//! option exists in exactly one place. The old per-topic files still work;
//! what the kdl file says wins.

use crate::config;
use kdl::{KdlDocument, KdlNode, KdlValue};
use std::collections::BTreeMap;

pub enum Kind {
    Flag,
    Num(f64, f64),
    Pick(&'static [&'static str]),
    Text,
}

pub struct Opt {
    pub section: &'static str,
    pub key: &'static str,
    pub kind: Kind,
    pub default: &'static str,
    pub doc: &'static str,
}

const fn o(section: &'static str, key: &'static str, kind: Kind, default: &'static str, doc: &'static str) -> Opt {
    Opt { section, key, kind, default, doc }
}

use Kind::{Flag, Num, Pick, Text};

pub const SECTIONS: &[(&str, &str)] = &[
    ("look", "colors and the clock"),
    ("window", "the frame around every window"),
    ("bar", "what the bar says"),
    ("sky", "the sky behind everything"),
    ("motion", "how the camera moves"),
    ("layout", "tiling the windows of a star"),
    ("idle", "locking and sleeping the screens when you leave"),
    ("panel", "the side panel: volume, toggles, notifications"),
    ("weather", "the weather card (add `weather` to the widgets file)"),
    ("calendar", "the agenda card (add `agenda` to the widgets file)"),
    ("yoru", "Yoru's lines and the menus"),
    ("launcher", "the app launcher"),
];

pub const OPTS: &[Opt] = &[
    o("look", "wallpaper", Text, "", "picture the palette is borrowed from (wallust reads it)"),
    o("look", "slideshow", Num(0.0, 1440.0), "0", "minutes between pictures: the next one in the wallpaper's folder recolors the sky; 0 is off"),
    o("look", "style", Pick(&["soft", "hard", "comp", "salient"]), "soft", "how boldly the picture colors the sky"),
    o("look", "contrast_guard", Flag, "on", "swap in cream text when a picture is too dim to read on"),
    o("look", "clock", Pick(&["24", "12"]), "24", "24 or 12 hour clock"),
    o("window", "radius", Num(0.0, 32.0), "14", "corner roundness, in pixels"),
    o("window", "border", Num(0.0, 8.0), "2", "frame thickness, in pixels"),
    o("window", "glow", Pick(&["off", "soft", "bright"]), "soft", "the bloom behind a window"),
    o("bar", "date", Flag, "on", "the date next to the clock"),
    o("bar", "stats", Flag, "on", "cpu and ram"),
    o("bar", "volume", Flag, "on", "the speaker level"),
    o("bar", "autohide", Flag, "off", "hide the bar until the pointer touches the top edge"),
    o("bar", "battery", Flag, "on", "the battery, where there is one"),
    o("bar", "dots", Flag, "on", "one dot per star in the middle; click one to fly there"),
    o("bar", "media", Flag, "on", "what is playing, next to the clock; click to play or pause"),
    o("bar", "quick", Flag, "on", "screenshot and microphone buttons"),
    o("bar", "net", Flag, "on", "network and bluetooth status; click opens the panel"),
    o("bar", "weather", Flag, "off", "the temperature, from wttr.in (uses the weather place and units)"),
    o("sky", "nebula", Num(0.0, 1.0), "0.4", "how much painted cloud the night shows"),
    o("sky", "shooting", Flag, "on", "shooting stars, now and then"),
    o("sky", "labels", Flag, "on", "star names in the overview"),
    o("sky", "threads", Flag, "on", "the lines joining stars in the overview"),
    o("motion", "pan_secs", Num(0.05, 3.0), "0.35", "seconds a pan between altitudes takes"),
    o("motion", "easing", Pick(&["drift", "glide", "float", "calm", "bounce"]), "drift", "how a pan settles"),
    o("motion", "reduce", Flag, "off", "pans become jumps"),
    o("layout", "tile", Flag, "off", "every star tiles its windows (Alt+t flips one star)"),
    o("layout", "max", Num(1.0, 8.0), "3", "windows a tiled star holds before the rest wait outside it"),
    o("layout", "gap", Num(0.0, 64.0), "10", "pixels between tiles and around them"),
    o("layout", "master", Num(0.2, 0.8), "0.55", "how much of the width the first window takes when others are beside it"),
    o("weather", "place", Text, "", "a city or place, like Lisbon; empty lets wttr.in guess from your address"),
    o("weather", "units", Pick(&["metric", "imperial"]), "metric", "degrees C or F"),
    o("calendar", "source", Text, "", "an .ics file or address (a calendar's private link); repeating events show once"),
    o("idle", "dim", Num(0.0, 86400.0), "0", "seconds without input until the screen dims; 0 is never"),
    o("idle", "lock", Num(0.0, 86400.0), "0", "seconds without input until the screen locks; 0 is never"),
    o("idle", "off", Num(0.0, 86400.0), "0", "seconds without input until the screens switch off; 0 is never"),
    o("panel", "corner", Pick(&["top-right", "top-left", "bottom-right", "bottom-left", "off"]), "top-right", "the screen corner that opens the panel when the pointer rests there; Alt+p always does"),
    o("panel", "weather", Flag, "off", "a weather card in the panel (asks wttr.in; uses the weather place and units)"),
    o("panel", "calendar", Flag, "off", "this month's calendar in the panel"),
    o("panel", "dwell", Num(0.0, 2000.0), "150", "milliseconds the pointer rests in the corner first"),
    o("yoru", "lines", Flag, "on", "the little quips on the lock screen and launcher"),
    o("yoru", "confirm", Flag, "on", "press twice to shut down or log out"),
    o("launcher", "style", Pick(&["list", "stars"]), "list", "apps as a list, or as stars"),
    o("launcher", "search", Pick(&["duckduckgo", "google", "startpage", "brave"]), "duckduckgo", "what a query that is no app asks"),
    o("launcher", "terminal", Text, "", "runs apps that want one; kitty if empty"),
];

/// Things a bind can do: name, fewest and most numbers it takes, what it does.
/// Anything else a bind needs goes through `spawn`.
pub const ACTIONS: &[(&str, usize, usize, &str)] = &[
    ("pan", 2, 2, "move the camera by dx dy"),
    ("zoom-reset", 0, 0, "back to zoom 1"),
    ("overview", 0, 0, "zoom out to the whole constellation"),
    ("goto", 1, 1, "fly to star N"),
    ("new-star", 0, 0, "add a star here"),
    ("delete-star", 0, 1, "delete this star, or star N"),
    ("relaunch", 0, 0, "restart the apps remembered for this star"),
    ("move-window", 2, 2, "nudge the focused window by dx dy"),
    ("resize-window", 2, 2, "resize the focused window by dw dh"),
    ("focus", 2, 2, "focus the neighbouring window: dx dy, each -1, 0 or 1"),
    ("cycle-window", 0, 0, "focus the next window"),
    ("switch", 0, 0, "open the window switcher (Alt+Tab)"),
    ("close-window", 0, 0, "close the focused window"),
    ("maximize", 0, 0, "maximize or restore the focused window"),
    ("tile", 0, 0, "tile or untile the windows of the star you are in"),
    ("help", 0, 0, "list every binding"),
    ("reload-config", 0, 0, "read seiza.kdl again"),
    ("quit", 0, 0, "leave Seiza"),
];

#[derive(Default)]
pub struct Conf {
    /// `file/key` -> value, as the old per-topic files would say it (flags are 1 or 0).
    pub values: BTreeMap<String, String>,
    /// key combo -> shell command line
    pub binds: Vec<(String, String)>,
    /// None when the file has no autostart block (the built-in programs start)
    pub autostart: Option<Vec<String>>,
    /// window rules, one line each: tab-separated `key=value` (app, title, star, mode, w, h)
    pub rules: Vec<String>,
    pub errors: Vec<String>,
}

fn file_of(section: &str) -> &str {
    if section == "window" { "frame" } else { section }
}

fn at(text: &str, offset: usize, msg: impl std::fmt::Display) -> String {
    format!("seiza.kdl:{}: {msg}", text[..offset.min(text.len())].matches('\n').count() + 1)
}

fn scalar(v: &KdlValue) -> String {
    match v {
        KdlValue::String(s) => s.clone(),
        KdlValue::Integer(i) => i.to_string(),
        KdlValue::Float(f) => f.to_string(),
        KdlValue::Bool(b) => b.to_string(),
        KdlValue::Null => String::new(),
    }
}

fn check(kind: &Kind, v: &str) -> Result<String, String> {
    match kind {
        Flag => match v {
            "on" | "true" | "yes" | "1" => Ok("1".into()),
            "off" | "false" | "no" | "0" => Ok("0".into()),
            _ => Err(format!("expected on or off, got `{v}`")),
        },
        Num(lo, hi) => match v.parse::<f64>() {
            Ok(n) if (*lo..=*hi).contains(&n) => Ok(v.into()),
            Ok(_) => Err(format!("{v} is out of range, use {lo} to {hi}")),
            Err(_) => Err(format!("expected a number from {lo} to {hi}, got `{v}`")),
        },
        Pick(list) if list.contains(&v) => Ok(v.into()),
        Pick(list) => Err(format!("`{v}` is not one of {}", list.join(", "))),
        Text => Ok(v.into()),
    }
}

/// One action node as the command line a bind runs.
fn action(node: &KdlNode, defs: &BTreeMap<String, String>) -> Result<String, String> {
    let name = node.name().value();
    if node.entries().iter().any(|e| e.name().is_some()) {
        return Err(format!("`{name}` takes plain values, not name=value"));
    }
    let args: Vec<String> = node.entries().iter().map(|e| scalar(e.value())).collect();
    if name == "spawn" {
        return if args.is_empty() { Err("spawn needs a command, like: spawn \"kitty\"".into()) } else { Ok(args.join(" ")) };
    }
    if let Some(cmd) = defs.get(name) {
        return if args.is_empty() { Ok(cmd.clone()) } else { Err(format!("`{name}` is one of your defs and takes no values")) };
    }
    let Some((_, lo, hi, _)) = ACTIONS.iter().find(|a| a.0 == name) else {
        return Err(format!("unknown action `{name}` (try `seiza-conf docs` for the list)"));
    };
    if !(*lo..=*hi).contains(&args.len()) {
        return Err(format!("`{name}` takes {} value(s), got {}", if lo == hi { lo.to_string() } else { format!("{lo} to {hi}") }, args.len()));
    }
    if let Some(bad) = args.iter().find(|a| a.parse::<f64>().is_err()) {
        return Err(format!("`{name}` wants numbers, got `{bad}`"));
    }
    Ok(std::iter::once(format!("seiza {name}")).chain(args).collect::<Vec<_>>().join(" "))
}

fn combo_ok(combo: &str) -> bool {
    let mut parts = combo.split('+').collect::<Vec<_>>();
    let key = parts.pop().unwrap_or("");
    !key.is_empty() && parts.iter().all(|m| ["alt", "shift", "ctrl", "control", "super", "logo", "mod4"].contains(&m.to_lowercase().as_str()))
}

pub fn parse(text: &str) -> Conf {
    let mut c = Conf::default();
    let doc = match KdlDocument::parse_v2(text) {
        Ok(d) => d,
        Err(e) => {
            for d in e.diagnostics {
                c.errors.push(at(text, d.span.offset(), format!("{}{}", d.message.unwrap_or_else(|| "syntax error".into()), d.help.map(|h| format!(" ({h})")).unwrap_or_default())));
            }
            return c;
        }
    };
    // defs first, so a bind can use one that is written further down; a def cannot use another def
    let mut defs = BTreeMap::new();
    for n in doc.nodes().iter().filter(|n| n.name().value() == "def") {
        let name = n.entries().first().map(|e| scalar(e.value()));
        let body: Result<Vec<String>, String> = n.children().map(|k| k.nodes().iter().map(|a| action(a, &BTreeMap::new())).collect()).unwrap_or(Ok(vec![]));
        match (name, body) {
            (Some(name), Ok(b)) if !b.is_empty() => { defs.insert(name, b.join("; ")); }
            (None, _) => c.errors.push(at(text, n.span().offset(), "def needs a name: def name { action; }")),
            (_, Err(e)) => c.errors.push(at(text, n.span().offset(), e)),
            _ => c.errors.push(at(text, n.span().offset(), "def has no actions")),
        }
    }
    for n in doc.nodes() {
        let (name, here) = (n.name().value(), n.span().offset());
        let kids = n.children().map(|k| k.nodes()).unwrap_or(&[]);
        match name {
            "def" => {}
            "binds" => for b in kids {
                let combo = b.name().value();
                let acts: Result<Vec<String>, String> = b.children().map(|k| k.nodes().iter().map(|a| action(a, &defs)).collect()).unwrap_or_else(|| Err("write the actions in braces: Alt+l { spawn \"kitty\"; }".into()));
                match acts {
                    _ if !combo_ok(combo) => c.errors.push(at(text, b.span().offset(), format!("`{combo}` is not a key combo, like Alt+Shift+l"))),
                    Ok(a) if !a.is_empty() => c.binds.push((combo.into(), a.join("; "))),
                    Ok(_) => c.errors.push(at(text, b.span().offset(), format!("{combo} does nothing"))),
                    Err(e) => c.errors.push(at(text, b.span().offset(), e)),
                }
            },
            "rule" => {
                let mut f: Vec<String> = vec![];
                let mut ok = true;
                for e in n.entries() {
                    let (Some(k), v) = (e.name().map(|k| k.value()), scalar(e.value())) else {
                        ok = false;
                        c.errors.push(at(text, e.span().offset(), "rule takes name=value pairs: rule app=\"steam\" star=\"games\""));
                        continue;
                    };
                    let good = match k {
                        "app" | "title" | "star" => !v.is_empty() && !v.contains(['\t', '\n']),
                        "mode" => v == "float" || v == "maximize",
                        "w" | "h" => v.parse::<u32>().is_ok_and(|n| n >= 100),
                        _ => false,
                    };
                    if !good {
                        ok = false;
                        c.errors.push(at(text, e.span().offset(), format!("rule: bad `{k}` (use app, title, star, mode float|maximize, w, h; sizes from 100)")));
                    } else {
                        f.push(format!("{k}={v}"));
                    }
                }
                let has = |k: &str| f.iter().any(|x| x.starts_with(&format!("{k}=")));
                if ok && !(has("app") || has("title")) {
                    ok = false;
                    c.errors.push(at(text, here, "rule needs app= or title= to say which windows it is for"));
                }
                if ok && has("w") != has("h") {
                    ok = false;
                    c.errors.push(at(text, here, "rule: give both w and h, or neither"));
                }
                if ok {
                    c.rules.push(f.join("\t"));
                }
            }
            "autostart" => {
                let mut list = vec![];
                for k in kids {
                    match action(k, &defs) {
                        Ok(l) if k.name().value() == "spawn" => list.push(l),
                        Ok(_) | Err(_) => c.errors.push(at(text, k.span().offset(), "autostart only takes `spawn \"command\"`")),
                    }
                }
                c.autostart = Some(list);
            }
            s if SECTIONS.iter().any(|x| x.0 == s) => for k in kids {
                let Some(opt) = OPTS.iter().find(|o| o.section == s && o.key == k.name().value()) else {
                    let valid = OPTS.iter().filter(|o| o.section == s).map(|o| o.key).collect::<Vec<_>>().join(", ");
                    c.errors.push(at(text, k.span().offset(), format!("unknown option `{s}.{}` (valid: {valid})", k.name().value())));
                    continue;
                };
                match k.entries() {
                    [e] if e.name().is_none() => match check(&opt.kind, &scalar(e.value())) {
                        Ok(v) => { c.values.insert(format!("{}/{}", file_of(s), opt.key), v); }
                        Err(m) => c.errors.push(at(text, e.span().offset(), format!("{s}.{}: {m}", opt.key))),
                    },
                    _ => c.errors.push(at(text, k.span().offset(), format!("{s}.{} takes exactly one value", opt.key))),
                }
            },
            other => c.errors.push(at(text, here, format!("unknown section `{other}` (sections: {}, binds, def, autostart, rule)", SECTIONS.iter().map(|s| s.0).collect::<Vec<_>>().join(", ")))),
        }
    }
    c
}

/// The parsed file, or None when there is no seiza.kdl. Errors ride along in `errors`.
pub fn load() -> Option<Conf> {
    Some(parse(&std::fs::read_to_string(config::path("seiza.kdl")).ok()?))
}

/// One part of the config in the old flat file format, for the compositor:
/// `keys` (`Alt+l = cmd`), `autostart` (one command a line), or a settings file such as `motion` (`key=value`).
pub fn export(c: &Conf, name: &str) -> String {
    match name {
        "rules" => c.rules.iter().map(|r| format!("{r}\n")).collect(),
        "keys" => c.binds.iter().map(|(k, v)| format!("{k} = {v}\n")).collect(),
        // an empty block means "start nothing", so say something the parser will skip
        "autostart" => c.autostart.iter().map(|l| if l.is_empty() { "#\n".into() } else { l.iter().map(|x| format!("{x}\n")).collect::<String>() }).collect(),
        _ => c.values.iter().filter_map(|(k, v)| Some(format!("{}={v}\n", k.strip_prefix(name)?.strip_prefix('/')?))).collect(),
    }
}

/// Writes `file/key` into seiza.kdl, keeping its comments and layout. Ok(false): there is no seiza.kdl, or the table has no such option.
pub fn set(file: &str, key: &str, val: &str) -> std::io::Result<bool> {
    let p = config::path("seiza.kdl");
    let Ok(text) = std::fs::read_to_string(&p) else { return Ok(false) };
    let sec = if file == "frame" { "window" } else { file };
    let Some(opt) = OPTS.iter().find(|o| o.section == sec && o.key == key) else { return Ok(false) };
    let mut doc = KdlDocument::parse_v2(&text).map_err(|_| std::io::Error::other("seiza.kdl has a syntax error; fix it first"))?;
    let bare = !val.is_empty() && !["true", "false", "null", "inf", "nan"].contains(&val) && (val.parse::<f64>().is_ok() || val.starts_with(|c: char| c.is_ascii_alphabetic()) && val.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    let val = if matches!(opt.kind, Flag) { if val == "1" { "on" } else { "off" } } else { val };
    let repr = if bare || matches!(opt.kind, Flag) { val.to_string() } else { format!("\"{}\"", val.replace('\\', "\\\\").replace('"', "\\\"")) };
    let snippet = |text: String| KdlDocument::parse_v2(&text).map(|d| d.nodes().to_vec()).map_err(|_| std::io::Error::other("could not write that value"));
    if doc.get(sec).is_none() {
        doc.nodes_mut().extend(snippet(format!("{sec} {{\n    {key} {repr}\n}}\n"))?);
    } else {
        let kids = doc.get_mut(sec).unwrap().ensure_children();
        match kids.get_mut(key).and_then(|n| n.entries_mut().first_mut()) {
            // keep the user's spacing and trailing comment, change only the value
            Some(e) => {
                e.set_value(val);
                if let Some(f) = e.format_mut() {
                    f.value_repr = repr;
                }
            }
            None => {
                kids.nodes_mut().retain(|n| n.name().value() != key);
                kids.nodes_mut().extend(snippet(format!("    {key} {repr}\n"))?);
            }
        }
    }
    let out = doc.to_string();
    KdlDocument::parse_v2(&out).map_err(|_| std::io::Error::other("seiza.kdl is laid out in a way I cannot add to; edit it by hand"))?;
    std::fs::write(p, out).map(|_| true)
}

/// A commented starting file: every option with its default and what it does.
pub fn template() -> String {
    let mut s = String::from("// Seiza's config. Every line is optional: what you leave out keeps its default.\n// Uncomment a line to change it. Reload with `seiza reload-config`; check with `seiza-conf check`.\n");
    for (sec, blurb) in SECTIONS {
        s += &format!("\n// {blurb}\n{sec} {{\n");
        for o in OPTS.iter().filter(|o| o.section == *sec) {
            let range = match o.kind {
                Flag => "on or off".into(),
                Num(lo, hi) => format!("{lo} to {hi}"),
                Pick(l) => l.join(" | "),
                Text => "text".into(),
            };
            s += &format!("    // {} ({range})\n    // {} {}\n", o.doc, o.key, if o.default.is_empty() { "\"\"".into() } else { o.default.to_string() });
        }
        s += "}\n";
    }
    s += "\n// Key bindings. Added to the built-in ones (`seiza help`, Alt+h, lists them); same combo replaces.\n// Modifiers: Alt Shift Ctrl Super. Actions:\n";
    for (name, _, _, doc) in ACTIONS {
        s += &format!("//   {name}: {doc}\n");
    }
    s += "//   spawn \"command\": run a command\n// binds {\n//     Alt+Return { spawn \"kitty\"; }\n//     Alt+g      { goto 1; }\n//     Alt+Shift+g { my-thing; }\n// }\n";
    s += "\n// Your own names for a few actions in a row (a def cannot use another def):\n// def my-thing { goto 1; spawn \"notify-send hi\"; }\n";
    s += "\n// Where a window goes when it opens (the first rule that matches wins). app is the app id\n// (seiza debug-windows shows them), title is a part of the title, star is a star's name,\n// mode is float or maximize, w and h a starting size.\n// rule app=\"steam\" star=\"games\" mode=\"maximize\"\n// rule app=\"pavucontrol\" w=700 h=500\n";
    s += "\n// Programs to start with the session. Leave the block out to start the built-in ones;\n// an empty block starts nothing.\n// autostart {\n//     spawn \"seiza-starfield\"\n//     spawn \"seiza-bar\"\n//     spawn \"seiza-widgets\"\n// }\n";
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn rules_become_tab_separated_lines_and_mistakes_are_named() {
        let c = parse("rule app=\"steam\" star=\"games\" mode=maximize\nrule title=\"Pick\" w=700 h=500");
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        assert_eq!(export(&c, "rules"), "app=steam\tstar=games\tmode=maximize\ntitle=Pick\tw=700\th=500\n");
        let c = parse("rule star=\"x\"\nrule app=a w=700\nrule app=a mode=tile\nrule steam");
        assert_eq!(c.errors.len(), 4, "{:?}", c.errors);
        assert!(c.rules.is_empty());
    }

    #[test]
    fn good_file_reads_and_exports_the_old_formats() {
        let c = parse("window { radius 20; glow bright }\nmotion { reduce on; pan_secs 0.5 }\nbinds { Alt+Shift+g { goto 1; spawn \"kitty -e fish\" } }\ndef both { focus 1 0; close-window }\nbinds { Alt+b { both } }\nautostart { spawn \"seiza-bar\" }");
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        assert_eq!(export(&c, "frame"), "glow=bright\nradius=20\n");
        assert_eq!(export(&c, "motion"), "pan_secs=0.5\nreduce=1\n");
        assert_eq!(export(&c, "keys"), "Alt+Shift+g = seiza goto 1; kitty -e fish\nAlt+b = seiza focus 1 0; seiza close-window\n");
        assert_eq!(export(&c, "autostart"), "seiza-bar\n");
    }

    #[test]
    fn mistakes_are_named_with_their_line_and_the_rest_still_loads() {
        let c = parse("window {\n  radius big\n  glow loud\n  radsius 3\n  border 4\n}\nbinds {\n  Alt+x { warp 1 }\n  Hyper+x { quit }\n  Alt+q { goto }\n  Alt+z { goto 2 }\n}\nforecast {}\nautostart {}");
        let all = c.errors.join("\n");
        for want in ["seiza.kdl:2: window.radius: expected a number", "seiza.kdl:3: window.glow: `loud` is not one of off, soft, bright", "seiza.kdl:4: unknown option `window.radsius` (valid: radius, border, glow)", "seiza.kdl:8: unknown action `warp`", "seiza.kdl:9: `Hyper+x` is not a key combo", "seiza.kdl:10: `goto` takes 1 value(s), got 0", "seiza.kdl:13: unknown section `forecast`"] {
            assert!(all.contains(want), "missing {want:?} in:\n{all}");
        }
        assert_eq!(c.errors.len(), 7, "{all}");
        assert_eq!(export(&c, "frame"), "border=4\n");
        assert_eq!(export(&c, "keys"), "Alt+z = seiza goto 2\n");
        assert_eq!(export(&c, "autostart"), "#\n");
    }

    #[test]
    fn a_syntax_error_is_reported_not_swallowed() {
        assert!(!parse("window { radius 5").errors.is_empty());
    }

    /// Run by the config test that owns XDG_CONFIG_HOME (tests share the environment).
    pub(crate) fn set_edits_in_place_and_keeps_comments(dir: &std::path::Path) {
        let file = dir.join("seiza/seiza.kdl");
        std::fs::write(&file, "// mine\nwindow {\n    radius 14 // round\n}\n").unwrap();
        assert!(set("frame", "radius", "20").unwrap());
        assert!(set("bar", "stats", "0").unwrap());
        assert!(!set("look", "nope", "1").unwrap());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("// mine") && text.contains("// round"), "{text}");
        let c = parse(&text);
        assert!(c.errors.is_empty(), "{:?}\n{text}", c.errors);
        assert_eq!(c.values["frame/radius"], "20");
        assert_eq!(c.values["bar/stats"], "0");
        assert_eq!(config::get("frame", "radius").as_deref(), Some("20"));
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn the_template_is_a_valid_file_that_sets_nothing() {
        let c = parse(&template());
        assert!(c.errors.is_empty(), "{:?}", c.errors);
        assert!(c.values.is_empty() && c.binds.is_empty() && c.autostart.is_none());
    }
}
