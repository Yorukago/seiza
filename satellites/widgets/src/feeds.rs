//! What the weather, agenda and updates cards show, from the network, from a calendar file and from pacman. Plain text in, plain text out.
use std::process::Command;

#[derive(Default, Clone, PartialEq, Debug)]
pub struct Weather {
    pub temp: String,  // "+12°C"
    pub sky: String,   // "Partly cloudy"
    pub place: String, // "Lisbon, Portugal"
    pub extra: String, // "humidity 71%  wind 5km/h"
}

/// wttr.in's one-line format `%t|%C|%l|%h|%w` (temperature, sky, place, humidity, wind): five fields.
pub fn parse_weather(out: &str) -> Option<Weather> {
    let f: Vec<&str> = out.trim().split('|').collect();
    // an error page or a rate-limit message is not five fields
    (f.len() == 5 && f.iter().all(|x| !x.is_empty() && x.len() < 80)).then(|| Weather {
        temp: f[0].replace('+', ""),
        sky: f[1].into(),
        // with no place asked for, wttr.in answers with coordinates
        place: if f[2].chars().all(|c| c.is_ascii_digit() || "+-., ".contains(c)) { "your location".into() } else { f[2].into() },
        extra: format!("humidity {}  wind {}", f[3], f[4].trim_start_matches(|c: char| !c.is_ascii_digit())),
    })
}

/// One fetch from wttr.in; `place` empty lets it guess from the address. None offline.
pub fn fetch_weather(place: &str, imperial: bool) -> Option<Weather> {
    let place: String = place.trim().chars().map(|c| if c.is_ascii_alphanumeric() || "-_,.".contains(c) { c.to_string() } else if c == ' ' { "+".into() } else { format!("%{:02X}", c as u32 & 0xFF) }).collect();
    let url = format!("https://wttr.in/{place}?{}&format={}", if imperial { "u" } else { "m" }, "%25t%7C%25C%7C%25l%7C%25h%7C%25w");
    let out = Command::new("curl").args(["-sf", "-m", "10", &url]).output().ok()?;
    parse_weather(&String::from_utf8_lossy(&out.stdout))
}

#[derive(Clone, PartialEq, Debug)]
pub struct Event {
    pub at: i64, // unix seconds
    pub all_day: bool,
    pub what: String,
}

fn tm_of(y: i32, mo: i32, d: i32, h: i32, mi: i32) -> libc::tm {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    (tm.tm_year, tm.tm_mon, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_isdst) = (y - 1900, mo - 1, d, h, mi, -1);
    tm
}

/// DTSTART as seconds: `20261105T140000Z` (UTC), `20261105T140000` (local) or `20261105` (a whole day). Returns (seconds, all_day).
fn when(v: &str) -> Option<(i64, bool)> {
    let n = |a: usize, b: usize| v.get(a..b)?.parse::<i32>().ok();
    let (y, mo, d) = (n(0, 4)?, n(4, 6)?, n(6, 8)?);
    if v.len() == 8 {
        return Some((unsafe { libc::mktime(&mut tm_of(y, mo, d, 0, 0)) } as i64, true));
    }
    let mut tm = tm_of(y, mo, d, n(9, 11)?, n(11, 13)?);
    let t = if v.ends_with('Z') { unsafe { libc::timegm(&mut tm) } } else { unsafe { libc::mktime(&mut tm) } };
    Some((t as i64, false))
}

/// The VEVENTs of an .ics file. Repeating events (RRULE) show only their first date.
// shortcut: no RRULE, no time zone ids (TZID is read as local time); use a client that exports a flat agenda if it matters.
pub fn parse_ics(text: &str) -> Vec<Event> {
    // lines folded with a leading space continue the one before
    let unfolded = text.replace("\r\n", "\n").replace("\n ", "").replace("\n\t", "");
    let (mut out, mut at, mut what) = (vec![], None, None);
    for line in unfolded.lines() {
        if line == "BEGIN:VEVENT" {
            (at, what) = (None, None);
        } else if let Some((key, val)) = line.split_once(':') {
            match key.split(';').next().unwrap_or("") {
                "DTSTART" => at = when(val.trim()),
                "SUMMARY" => what = Some(val.replace("\\,", ",").replace("\\n", " ")),
                "END" if val == "VEVENT" => {
                    if let (Some((at, all_day)), Some(what)) = (at.take(), what.take()) {
                        out.push(Event { at, all_day, what });
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// The next `max` events that have not ended (an hour is assumed for a timed one; a whole day lasts the day), soonest first.
pub fn upcoming(mut events: Vec<Event>, now: i64, max: usize) -> Vec<Event> {
    events.retain(|e| e.at + if e.all_day { 86400 } else { 3600 } > now);
    events.sort_by_key(|e| e.at);
    events.truncate(max);
    events
}

/// "Thu 05  14:00" or "Thu 05  all day", in local time.
pub fn when_text(e: &Event) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = e.at as libc::time_t;
    unsafe { libc::localtime_r(&t, &mut tm) };
    let mut buf = [0u8; 32];
    let fmt = if e.all_day { c"%a %d  all day" } else { c"%a %d  %H:%M" };
    let n = unsafe { libc::strftime(buf.as_mut_ptr() as *mut libc::c_char, buf.len(), fmt.as_ptr(), &tm) };
    String::from_utf8_lossy(&buf[..n]).to_lowercase()
}

/// The calendar file: a path, or an http(s) address that is fetched with curl.
pub fn read_calendar(src: &str) -> Option<String> {
    let src = src.trim().replace('~', &std::env::var("HOME").unwrap_or_default());
    if src.starts_with("http://") || src.starts_with("https://") {
        let out = Command::new("curl").args(["-sfL", "-m", "20", &src]).output().ok()?;
        return out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned());
    }
    std::fs::read_to_string(src).ok()
}

/// The package names waiting for an upgrade: from `checkupdates` (the repos) and `yay -Qua` (the AUR).
#[derive(Clone, PartialEq, Debug)]
pub struct Updates {
    pub repo: Vec<String>,
    pub aur: Vec<String>,
}

/// The names from lines like "firefox 160.0-1 -> 161.0-1"; anything else (a progress line, a warning) is skipped.
pub fn parse_updates(out: &str) -> Vec<String> {
    out.lines().filter(|l| l.contains(" -> ")).filter_map(|l| l.split_whitespace().next()).map(String::from).collect()
}

/// None when the repo check cannot run (no pacman-contrib, or no network); yay is optional.
pub fn fetch_updates() -> Option<Updates> {
    let repo = Command::new("checkupdates").output().ok()?;
    if !matches!(repo.status.code(), Some(0 | 2)) {
        return None; // 0: updates waiting, 2: none, anything else: the check itself failed
    }
    let aur = Command::new("yay").arg("-Qua").output().map(|o| parse_updates(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default();
    Some(Updates { repo: parse_updates(&String::from_utf8_lossy(&repo.stdout)), aur })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weather_lines_parse_and_errors_do_not() {
        let w = parse_weather("+12°C|Partly cloudy|Lisbon, Portugal|71%|↗5km/h\n").unwrap();
        assert_eq!((w.temp.as_str(), w.sky.as_str(), w.extra.as_str()), ("12°C", "Partly cloudy", "humidity 71%  wind 5km/h"));
        assert_eq!(parse_weather("+9°C|Clear|38.705700,-9.135900|80%|→3km/h").unwrap().place, "your location");
        assert!(parse_weather("Unknown location; please try ~53.8,10.7").is_none());
        assert!(parse_weather("").is_none());
    }

    #[test]
    fn ics_events_are_read_folded_lines_and_all() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261105T140000Z\r\nSUMMARY:Dentist\\, with a very\r\n  long name\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261106\r\nSUMMARY:Holiday\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nSUMMARY:No date\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let e = parse_ics(ics);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].what, "Dentist, with a very long name");
        assert!(!e[0].all_day && e[1].all_day);
        assert_eq!(e[0].at, 1793887200, "2026-11-05 14:00 UTC");
    }

    #[test]
    fn upcoming_skips_the_past_and_sorts() {
        let ev = |at, what: &str| Event { at, all_day: false, what: what.into() };
        let u = upcoming(vec![ev(20000, "later"), ev(100, "past"), ev(9000, "ongoing")], 10000, 5);
        assert_eq!(u.iter().map(|e| e.what.as_str()).collect::<Vec<_>>(), ["ongoing", "later"]);
        assert_eq!(upcoming(vec![ev(20000, "a"), ev(30000, "b")], 10000, 1).len(), 1);
    }

    #[test]
    fn updates_are_the_names_on_lines_with_an_arrow() {
        let out = "firefox 160.0-1 -> 161.0-1\n:: Checking for updates\nlinux-cachyos 7.3.4-1 -> 7.3.5-1\n\nnot a package line\n";
        assert_eq!(parse_updates(out), ["firefox", "linux-cachyos"]);
        assert!(parse_updates("").is_empty());
    }
}
