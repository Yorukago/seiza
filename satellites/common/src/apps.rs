//! Installed apps from `.desktop` entries, and the launcher's search.

use std::{collections::HashSet, path::PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub name: String,
    pub exec: String, // the command line to run, field codes stripped, terminal apps wrapped
    pub extra: String, // lower-cased generic name and keywords, searched after the name
    pub group: Group,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Group {
    Make,
    Web,
    Play,
    System,
}

pub const GROUPS: [(Group, &str); 4] = [(Group::Make, "make"), (Group::Web, "web"), (Group::Play, "play"), (Group::System, "system")];

fn group_of(categories: &str) -> Group {
    let has = |c: &str| categories.split(';').any(|x| x == c);
    if has("Network") {
        Group::Web
    } else if has("AudioVideo") || has("Audio") || has("Video") || has("Game") {
        Group::Play
    } else if has("Development") || has("Graphics") || has("Office") || has("Education") || has("Science") {
        Group::Make
    } else {
        Group::System
    }
}

/// Parse one `.desktop` file. None for entries that should not be listed.
pub fn parse_desktop(text: &str) -> Option<App> {
    let (mut name, mut exec, mut generic, mut keywords, mut cats) = (None, None, "", "", "");
    let (mut hidden, mut terminal, mut is_app) = (false, false, false);
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "Name" => name = Some(v),
            "Exec" => exec = Some(v),
            "GenericName" => generic = v,
            "Keywords" => keywords = v,
            "Categories" => cats = v,
            "NoDisplay" | "Hidden" => hidden |= v == "true",
            "Terminal" => terminal = v == "true",
            "Type" => is_app = v == "Application",
            _ => {}
        }
    }
    if hidden || !is_app {
        return None;
    }
    let exec = strip_codes(exec?);
    Some(App {
        name: name?.to_string(),
        exec: if terminal { format!("{} -e {exec}", terminal_command()) } else { exec },
        extra: format!("{generic};{keywords}").to_lowercase(),
        group: group_of(cats),
    })
}

/// What runs apps that want a terminal: `terminal=` in ~/.config/seiza/launcher, kitty if it is not there
/// or is anything but a plain command name or path (it ends up on a shell line).
pub fn terminal_command() -> String {
    static TERM: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TERM.get_or_init(|| {
        crate::config::get("launcher", "terminal").filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || "_-./".contains(c))).unwrap_or_else(|| "kitty".into())
    })
    .clone()
}

/// Drop `%f %U ...` field codes; `%%` is a literal percent.
fn strip_codes(exec: &str) -> String {
    let mut out = String::new();
    let mut it = exec.chars();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
        } else if it.next() == Some('%') {
            out.push('%');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every app in the user's and the system's application dirs, A-Z; the first
/// file with a given id wins, so the user's copy overrides the system's.
pub fn load_apps() -> Vec<App> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data_home = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| format!("{home}/.local/share"));
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let dirs: Vec<PathBuf> = std::iter::once(data_home).chain(data_dirs.split(':').map(String::from)).map(|d| PathBuf::from(d).join("applications")).collect();
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if !id.ends_with(".desktop") || !seen.insert(id) {
                continue;
            }
            if let Some(app) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_desktop(&t)) {
                apps.push(app);
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.name == b.name && a.exec == b.exec);
    apps
}

/// Lower is better. Prefix beats substring beats subsequence ("vsc" finds
/// "Visual Studio Code"); generic name and keywords only count as a last resort.
fn score(app: &App, q: &str) -> Option<u32> {
    let name = app.name.to_lowercase();
    if name.starts_with(q) {
        return Some(0);
    }
    if name.contains(q) {
        return Some(1);
    }
    let mut it = name.chars();
    if q.chars().count() >= 3 && q.chars().all(|c| it.any(|n| n == c)) { // two letters would match nearly everything
        return Some(2);
    }
    app.extra.contains(q).then_some(3)
}

/// Indices of the apps matching `query`, best first, ties A-Z (`apps` is already A-Z).
pub fn rank(apps: &[App], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    let mut hits: Vec<(u32, usize)> = apps.iter().enumerate().filter_map(|(i, a)| Some((if q.is_empty() { 0 } else { score(a, &q)? }, i))).collect();
    hits.sort();
    hits.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FF: &str = "[Desktop Entry]\nType=Application\nName=Firefox\nName[pt]=Raposa\nGenericName=Web Browser\nExec=firefox %u\nCategories=Network;WebBrowser;\nKeywords=internet;www;\n[Desktop Action new-window]\nName=New Window\nExec=firefox --new-window\n";

    #[test]
    fn parses_the_main_group_only() {
        let a = parse_desktop(FF).unwrap();
        assert_eq!((a.name.as_str(), a.exec.as_str(), a.group), ("Firefox", "firefox", Group::Web));
        assert!(a.extra.contains("internet"));
    }

    #[test]
    fn skips_hidden_and_non_apps_and_wraps_terminal_apps() {
        assert!(parse_desktop("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n").is_none());
        assert!(parse_desktop("[Desktop Entry]\nType=Link\nName=X\nExec=x\n").is_none());
        let t = parse_desktop("[Desktop Entry]\nType=Application\nName=htop\nExec=htop\nTerminal=true\n").unwrap();
        assert_eq!(t.exec, format!("{} -e htop", terminal_command()));
        assert_eq!(strip_codes("a %f  b %% %U"), "a b %");
    }

    fn app(name: &str, extra: &str) -> App {
        App { name: name.into(), exec: name.into(), extra: extra.into(), group: Group::System }
    }

    #[test]
    fn ranks_prefix_then_substring_then_subsequence_then_keywords() {
        let apps = [app("Fire Alarm", ""), app("Firefox", ""), app("Campfire", ""), app("Files", ""), app("Visual Studio Code", ""), app("Browser", "firefox;web")];
        let names = |q: &str| rank(&apps, q).iter().map(|&i| apps[i].name.as_str()).collect::<Vec<_>>();
        assert_eq!(names("fire"), ["Fire Alarm", "Firefox", "Campfire", "Browser"]);
        assert_eq!(names("vsc"), ["Visual Studio Code"]);
        assert_eq!(names("").len(), 6);
        assert!(names("zzz").is_empty());
    }
}
