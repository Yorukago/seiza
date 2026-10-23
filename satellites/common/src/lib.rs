//! What every Seiza satellite needs: the compositor's state stream and a way
//! to send it commands. Both talk to `$XDG_RUNTIME_DIR/seiza.sock`.
pub mod apps;
pub mod art;
pub mod config;
pub mod constellation;
pub mod draw;
pub mod figure;
pub mod palette;
pub mod sys;

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Star {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub windows: u64,
}

/// One state line from `seiza subscribe`. On screen, a world point is at
/// `world * zoom - cam`.
#[derive(Clone, Debug)]
pub struct State {
    pub constellation: String,
    pub current: String,
    pub overview: bool,
    pub zoom: f64,
    pub cam: (f64, f64),
    pub screen: (f64, f64),
    pub stars: Vec<Star>,
    pub links: Vec<(usize, usize)>, // threads joining stars, as indexes into `stars`
}

impl State {
    pub fn parse(line: &str) -> Option<State> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let num = |a: &serde_json::Value, d: f64| a.as_f64().unwrap_or(d);
        Some(State {
            constellation: v["constellation"].as_str()?.to_string(),
            current: v["current"].as_str().unwrap_or("").to_string(),
            overview: v["overview"].as_bool().unwrap_or(false),
            zoom: num(&v["zoom"], 1.0),
            cam: (num(&v["cam"][0], 0.0), num(&v["cam"][1], 0.0)),
            screen: (num(&v["screen"][0], 1920.0), num(&v["screen"][1], 1080.0)),
            stars: v["stars"]
                .as_array()?
                .iter()
                .map(|s| Star {
                    name: s["name"].as_str().unwrap_or("").to_string(),
                    x: num(&s["x"], 0.0),
                    y: num(&s["y"], 0.0),
                    windows: s["windows"].as_u64().unwrap_or(0),
                })
                .collect(),
            links: v["links"].as_array().map_or(vec![], |l| l.iter().filter_map(|p| Some((p[0].as_u64()? as usize, p[1].as_u64()? as usize))).collect()),
        })
    }
}

/// Star colors, in creation order: the same palette the compositor draws its
/// overview markers with, so satellites can match them. Gold, ice and cream:
/// pink and crimson are kept for tiny moments.
pub const STAR_COLORS: [[f32; 3]; 6] = [
    [0.914, 0.831, 0.561], // seiza gold
    [0.663, 0.847, 0.961], // seiza ice
    [0.949, 0.910, 0.788], // moon cream
    [0.788, 0.635, 0.294], // yoru trim gold
    [0.914, 0.831, 0.561],
    [0.663, 0.847, 0.961],
];

/// The clock's format: 24 hours unless `clock=12` is in ~/.config/seiza/look. Read once.
pub fn clock_fmt() -> &'static std::ffi::CStr {
    static TWELVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *TWELVE.get_or_init(|| config::get("look", "clock").as_deref() == Some("12")) { c"%I:%M %p" } else { c"%H:%M" }
}

/// A local-time string, `fmt` being strftime's.
pub fn local_time(fmt: &std::ffi::CStr) -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let mut buf = [0u8; 64];
        let n = libc::strftime(buf.as_mut_ptr() as *mut _, buf.len(), fmt.as_ptr(), &tm);
        String::from_utf8_lossy(&buf[..n]).into_owned()
    }
}

/// The state Seiza is in right now: one line from `seiza subscribe`, then let go. None if Seiza
/// is not there or does not answer in a second.
pub fn snapshot() -> Option<State> {
    let mut s = UnixStream::connect(sock_path()).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    s.write_all(b"seiza subscribe\n").ok()?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).ok()?;
    State::parse(&line)
}

fn sock_path() -> String {
    format!("{}/seiza.sock", std::env::var("XDG_RUNTIME_DIR").unwrap_or_default())
}

/// Call `on_state` for every state line, reconnecting if the socket goes away.
/// Blocks; returns when `on_state` returns false (run it on its own thread).
pub fn subscribe(mut on_state: impl FnMut(State) -> bool) {
    loop {
        if let Ok(mut s) = UnixStream::connect(sock_path()) {
            if s.write_all(b"seiza subscribe\n").is_ok() {
                for line in BufReader::new(s).lines().map_while(Result::ok) {
                    if let Some(st) = State::parse(&line) {
                        if !on_state(st) {
                            return;
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
}

/// Send one command (`"seiza goto 2"`, or a shell line) and return the reply.
pub fn command(line: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(sock_path())?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(format!("{line}\n").as_bytes())?;
    let mut reply = String::new();
    s.read_to_string(&mut reply)?;
    Ok(reply.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_state_line() {
        let st = State::parse(r#"{"constellation":"default","current":"star 0","overview":false,"zoom":1.0000,"cam":[0.0,-3.5],"screen":[1920,1080],"stars":[{"name":"star 0","x":960,"y":540,"w":1920,"h":1080,"windows":2}]}"#).unwrap();
        assert_eq!(st.constellation, "default");
        assert_eq!(st.cam, (0.0, -3.5));
        assert_eq!(st.stars[0].windows, 2);
        assert!(st.links.is_empty(), "an older stream without links still parses");
        let st = State::parse(r#"{"constellation":"c","stars":[{"name":"a","x":0,"y":0},{"name":"b","x":1,"y":1}],"links":[[0,1],[1,0]]}"#).unwrap();
        assert_eq!(st.links, [(0, 1), (1, 0)]);
        assert!(State::parse("not json").is_none());
    }
}
