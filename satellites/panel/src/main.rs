//! seiza-panel: a side panel with volume and brightness sliders, Wi-Fi / Bluetooth / microphone toggles
//! and the notifications seiza-notify has shown. Opens from the top-right hot corner (`panel` in
//! seiza.kdl) or Alt+d; running it again closes it, `seiza-panel open` only opens. Esc, or the pointer
//! leaving it, closes it. `seiza-panel --dump out.ppm` draws one frame.
//! shortcut: the toggles and sliders are read once on open and after each click (wpctl, nmcli, bluetoothctl,
//! brightnessctl are run synchronously), so a change made elsewhere while it is open shows next time.
use std::process::Command;

use seiza_common::{
    draw::{load_font, Canvas, Rgba, Text},
    palette::{palette, rgba},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::client::{
        globals::registry_queue_init,
        protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        Connection, QueueHandle,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

const W: u32 = 360;
const BTN_LEFT: u32 = 0x110;
const PAD: f32 = 20.0;
const TRACK_X: f32 = 64.0;
const TRACK_W: f32 = W as f32 - 64.0 - 56.0;
const NOTE_H: f32 = 56.0;
const TILE_H: f32 = 84.0;
const TILE_PITCH: f32 = 94.0;

fn sh(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
}

/// "Volume: 0.42 [MUTED]" -> (0.42, true)
fn parse_volume(s: &str) -> Option<(f32, bool)> {
    Some((s.split_whitespace().nth(1)?.parse().ok()?, s.contains("[MUTED]")))
}

#[derive(Clone, PartialEq, Debug)]
struct Note {
    at: u64,
    app: String,
    summary: String,
    body: String,
}

fn parse_notes(s: &str) -> Vec<Note> {
    let mut v: Vec<Note> = s
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(4, '\t');
            Some(Note { at: f.next()?.parse().ok()?, app: f.next()?.into(), summary: f.next()?.into(), body: f.next().unwrap_or("").into() })
        })
        .collect();
    v.reverse(); // newest first
    v
}

fn notes_path() -> std::path::PathBuf {
    std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("seiza-notifications")
}

fn ago(secs: u64) -> String {
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

fn clip(t: &mut Text, s: &str, max: f32) -> String {
    if t.width(s) <= max {
        return s.into();
    }
    let mut out: String = s.into();
    while !out.is_empty() && t.width(&out) + 10.0 > max {
        out.pop();
    }
    out.trim_end().to_string() + "…"
}

fn uptime() -> String {
    let s: f64 = std::fs::read_to_string("/proc/uptime").ok().and_then(|u| u.split_whitespace().next()?.parse().ok()).unwrap_or(0.0);
    let m = (s / 60.0) as u64;
    if m >= 60 { format!("up {}h {}m", m / 60, m % 60) } else { format!("up {m}m") }
}

/// (year, month, day) of today, the month's name, and the weekday of its 1st (Monday = 0).
#[derive(Clone, PartialEq, Debug)]
struct Cal {
    day: u32,
    days: u32,
    first: u32,
    title: String,
}

fn cal_now() -> Option<Cal> {
    let t = seiza_common::local_time(c"%Y %m %d");
    let mut n = t.split(' ').filter_map(|v| v.parse::<i32>().ok());
    let (y, m, d) = (n.next()?, n.next()? as u32, n.next()? as u32);
    Some(cal_of(y, m, d, seiza_common::local_time(c"%B %Y")))
}

fn cal_of(y: i32, m: u32, d: u32, title: String) -> Cal {
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31][m as usize - 1];
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4]; // Sakamoto: weekday of the 1st, Sunday = 0
    let yy = if m < 3 { y - 1 } else { y };
    let sun0 = (yy + yy / 4 - yy / 100 + yy / 400 + t[m as usize - 1] + 1).rem_euclid(7) as u32;
    Cal { day: d, days, first: (sun0 + 6) % 7, title }
}

/// The temperature and sky from wttr.in, cached in the runtime dir. A stale cache starts a background
/// `curl` for the next time the panel opens, so opening never waits on the network.
fn weather() -> String {
    let file = std::path::Path::new(&std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("seiza-weather");
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if age.is_none_or(|a| a.as_secs() > 1200) {
        let place: String = seiza_common::config::get("weather", "place").unwrap_or_default().trim().chars().map(|c| if c.is_ascii_alphanumeric() || "-_,.".contains(c) { c } else { '+' }).collect();
        let units = if seiza_common::config::get("weather", "units").as_deref() == Some("imperial") { "u" } else { "m" };
        let url = format!("https://wttr.in/{place}?{units}&format=%25t%7C%25C");
        // -f: an error page is not saved over a good cache
        let _ = Command::new("sh").args(["-c", "curl -sf -m 10 \"$1\" -o \"$2.new\" && mv \"$2.new\" \"$2\"", "sh", &url, &file.to_string_lossy()]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    }
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    match text.trim().split_once('|') {
        Some((t, sky)) if t.len() < 12 && sky.len() < 40 => format!("{}  {}", t.replace('+', ""), sky),
        _ => String::new(),
    }
}

/// What the panel shows and does. Reading and writing the system is here; drawing is `render`.
struct State {
    vol: f32,
    muted: bool,
    bright: Option<f32>, // None: no backlight on this machine
    wifi: bool,
    bt: bool,
    mic_on: bool,
    notes: Vec<Note>,
    now: u64,
    weather: String, // "12°C  Partly cloudy", empty when off or not fetched yet
    cal: Option<Cal>,
    hidden: Vec<String>, // tile ids the user hid in edit mode
    edit: bool,
}

impl State {
    fn read() -> State {
        let (vol, muted) = sh("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]).and_then(|s| parse_volume(&s)).unwrap_or((0.0, false));
        let bright = std::fs::read_dir("/sys/class/backlight").ok().and_then(|mut d| d.next()?.ok()).and_then(|e| {
            let num = |f: &str| std::fs::read_to_string(e.path().join(f)).ok()?.trim().parse::<f32>().ok();
            Some(num("brightness")? / num("max_brightness")?.max(1.0))
        });
        State {
            vol,
            muted,
            bright,
            wifi: sh("nmcli", &["radio", "wifi"]).is_some_and(|s| s.trim() == "enabled"),
            bt: sh("bluetoothctl", &["show"]).is_some_and(|s| s.contains("Powered: yes")),
            mic_on: sh("wpctl", &["get-volume", "@DEFAULT_AUDIO_SOURCE@"]).and_then(|s| parse_volume(&s)).is_some_and(|(_, m)| !m),
            notes: std::fs::read_to_string(notes_path()).map(|s| parse_notes(&s)).unwrap_or_default(),
            now: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            weather: if seiza_common::config::flag("panel", "weather", false) { weather() } else { String::new() },
            cal: if seiza_common::config::flag("panel", "calendar", false) { cal_now() } else { None },
            hidden: seiza_common::config::get("panel", "hidden").unwrap_or_default().split(',').filter(|t| !t.is_empty()).map(String::from).collect(),
            edit: false,
        }
    }

    fn set_volume(&mut self, v: f32) {
        self.vol = v.clamp(0.0, 1.0);
        run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.2}", self.vol)]);
    }

    fn set_brightness(&mut self, v: f32) {
        let v = v.clamp(0.02, 1.0); // never all the way off: that is a black screen with no way back
        self.bright = Some(v);
        run("brightnessctl", &["set", &format!("{}%", (v * 100.0).round())]);
    }

    /// Tiles on show: all of them while editing, else the ones not hidden.
    fn shown(&self) -> Vec<usize> {
        (0..TILES.len()).filter(|&i| self.edit || !self.hidden.iter().any(|h| h == TILES[i].0)).collect()
    }

    /// Edit mode: hide or show tile `i` and remember it.
    fn flip_hidden(&mut self, i: usize) {
        let id = TILES[i].0;
        match self.hidden.iter().position(|h| h == id) {
            Some(p) => drop(self.hidden.remove(p)),
            None => self.hidden.push(id.into()),
        }
        let _ = seiza_common::config::set("panel", "hidden", &self.hidden.join(","));
    }

    /// Press tile `i`; true if it started a program (the panel then gets out of the way).
    fn toggle(&mut self, i: usize) -> bool {
        match i {
            0 => {
                self.wifi = !self.wifi;
                run("nmcli", &["radio", "wifi", if self.wifi { "on" } else { "off" }]);
            }
            1 => {
                self.bt = !self.bt;
                run("bluetoothctl", &["power", if self.bt { "on" } else { "off" }]);
            }
            2 => {
                self.mic_on = !self.mic_on;
                run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SOURCE@", if self.mic_on { "0" } else { "1" }]);
            }
            _ => {
                let (cmd, args) = ACTIONS[i - 3];
                let _ = Command::new(cmd).args(args).spawn();
                return true;
            }
        }
        false
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Power,
    Mute,
    Volume,
    Brightness,
    Tile(usize),
    Clear,
    Edit,
}

type Rect = (f32, f32, f32, f32);

fn inside(r: Rect, x: f32, y: f32) -> bool {
    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
}

fn find(hits: &[(Rect, Hit)], x: f32, y: f32) -> Option<Hit> {
    hits.iter().find(|(r, _)| inside(*r, x, y)).map(|(_, h)| *h)
}

/// 0..=1 along a slider track for a pointer at `x`.
fn along(x: f32) -> f32 {
    ((x - TRACK_X) / TRACK_W).clamp(0.0, 1.0)
}

/// (id, label). The first three are switches; the rest run `ACTIONS` (same order, from index 3).
const TILES: [(&str, &str); 8] = [("wifi", "Wi-Fi"), ("bt", "Bluetooth"), ("mic", "Mic"), ("shot", "Screenshot"), ("rec", "Record"), ("clip", "Clipboard"), ("sky", "Overview"), ("reload", "Reload")];
const ACTIONS: [(&str, &[&str]); 5] = [
    ("seiza-shot", &["area"]),
    ("seiza-shot", &["record", "screen"]),
    ("seiza-launcher", &["--clip"]),
    ("seiza", &["overview"]),
    ("seiza", &["reload-config"]),
];

fn render(st: &State, h: u32, hover: Option<Hit>) -> (Canvas, Vec<(Rect, Hit)>) {
    let (w, hh) = (W as usize, h as usize);
    let mut cv = Canvas { w, h: hh, px: vec![0; w * hh] };
    let p = palette();
    let (ink, dim, gold): (Rgba, Rgba, Rgba) = (rgba(p.text, 1.0), rgba(p.text, 0.55), rgba(p.star, 1.0));
    let mut hits = vec![];
    let (mut big, mut mid, mut small) = (Text::new(load_font(), 20.0), Text::new(load_font(), 14.0), Text::new(load_font(), 12.0));
    cv.round_rect(0.0, 0.0, W as f32, h as f32, 24.0, rgba(p.panel, 0.96));

    // header: the clock-ish line and the power button
    cv.star5(PAD + 8.0, 34.0, 8.0, gold);
    big.draw(&mut cv, PAD + 26.0, 41.0, "seiza", ink);
    mid.draw(&mut cv, PAD + 26.0 + big.width("seiza") + 14.0, 40.0, &uptime(), dim);
    let eb = (W as f32 - PAD - 40.0 - 8.0 - 56.0, 20.0, 56.0, 28.0);
    cv.round_rect(eb.0, eb.1, eb.2, eb.3, 14.0, if st.edit { rgba(p.star, 0.35) } else { rgba(p.text, if hover == Some(Hit::Edit) { 0.18 } else { 0.07 }) });
    let ew = small.width(if st.edit { "done" } else { "edit" });
    small.draw(&mut cv, eb.0 + (eb.2 - ew) / 2.0, eb.1 + 18.0, if st.edit { "done" } else { "edit" }, ink);
    hits.push((eb, Hit::Edit));
    let pw = (W as f32 - PAD - 40.0, 14.0, 40.0, 40.0);
    cv.round_rect(pw.0, pw.1, pw.2, pw.3, 20.0, rgba(p.text, if hover == Some(Hit::Power) { 0.18 } else { 0.07 }));
    cv.icon_power(pw.0 + 20.0, pw.1 + 20.0, 9.0, ink);
    hits.push((pw, Hit::Power));

    // sliders
    let mut y = 80.0;
    cv.round_rect(10.0, y - 6.0, W as f32 - 20.0, if st.bright.is_some() { 98.0 } else { 52.0 }, 22.0, rgba(p.text, 0.05));
    let mut slider = |cv: &mut Canvas, y: f32, frac: f32, muted: bool, hit: Hit, hits: &mut Vec<(Rect, Hit)>| {
        let c = if muted { rgba(p.star, 0.4) } else { gold };
        let cy = y + 20.0;
        if hit == Hit::Mute {
            cv.icon_speaker(PAD + 14.0, cy, 11.0, frac * 3.0, muted, c);
            hits.push(((PAD, y, 40.0, 40.0), Hit::Mute));
        } else {
            cv.icon_sun(PAD + 14.0, cy, 11.0, frac * 4.0 * std::f32::consts::PI, c);
        }
        cv.round_rect(TRACK_X, cy - 3.0, TRACK_W, 6.0, 3.0, rgba(p.text, 0.18));
        let fill = (TRACK_W * frac).max(6.0);
        cv.round_rect(TRACK_X, cy - 3.0, fill, 6.0, 3.0, c);
        cv.star5(TRACK_X + fill, cy, 8.0, c);
        let label = format!("{}", (frac * 100.0).round());
        let lw = small.width(&label);
        small.draw(cv, W as f32 - PAD - lw, cy + 4.0, &label, dim);
        hits.push(((TRACK_X - 8.0, y, TRACK_W + 16.0, 40.0), if hit == Hit::Mute { Hit::Volume } else { Hit::Brightness }));
    };
    slider(&mut cv, y, st.vol, st.muted, Hit::Mute, &mut hits);
    y += 46.0;
    if let Some(b) = st.bright {
        slider(&mut cv, y, b, false, Hit::Brightness, &mut hits);
        y += 46.0;
    }

    // toggle tiles
    y += 10.0;
    let tw = (W as f32 - 2.0 * PAD - 20.0) / 3.0;
    let shown = st.shown();
    for (n, &i) in shown.iter().enumerate() {
        let (id, name) = TILES[i];
        let hid = st.hidden.iter().any(|h| h == id);
        let on = i < 3 && [st.wifi, st.bt, st.mic_on][i];
        let r = (PAD + (n % 3) as f32 * (tw + 10.0), y + (n / 3) as f32 * TILE_PITCH, tw, TILE_H);
        let lit = hover == Some(Hit::Tile(i));
        // a lit switch is solid gold with dark ink, like a pressed key
        let (base, fg): (Rgba, Rgba) = if on { (rgba(p.star, if lit { 1.0 } else { 0.92 }), rgba(p.sky, 1.0)) } else { (rgba(p.text, if lit { 0.14 } else { 0.07 }), rgba(p.text, if hid { 0.3 } else { 0.85 })) };
        cv.round_rect(r.0, r.1, r.2, r.3, 20.0, base);
        let (ix, iy) = (r.0 + tw / 2.0, r.1 + 26.0);
        match id {
            "wifi" => cv.icon_wifi(ix, iy, 11.0, fg),
            "bt" => cv.icon_bluetooth(ix, iy, 11.0, fg),
            "mic" => cv.icon_mic(ix, iy, 11.0, !on, fg),
            "shot" => cv.icon_frame(ix, iy, 10.0, fg),
            "rec" => cv.icon_record(ix, iy, 11.0, fg),
            "clip" => cv.icon_clipboard(ix, iy, 11.0, fg),
            "sky" => cv.icon_constellation(ix, iy, 12.0, fg),
            _ => cv.icon_restart(ix, iy, 11.0, fg),
        }
        let nw = mid.width(name);
        mid.draw(&mut cv, r.0 + (tw - nw) / 2.0, r.1 + 59.0, name, fg);
        let state = if st.edit { if hid { "hidden" } else { "shown" } } else if i < 3 { if on { "on" } else { "off" } } else { "" };
        let sw = small.width(state);
        small.draw(&mut cv, r.0 + (tw - sw) / 2.0, r.1 + 75.0, state, if on { rgba(p.sky, 0.7) } else { dim });
        hits.push((r, Hit::Tile(i)));
    }
    y += (shown.len().div_ceil(3).max(1) as f32) * TILE_PITCH - 10.0 + 22.0;

    // weather
    if !st.weather.is_empty() {
        cv.round_rect(PAD, y, W as f32 - 2.0 * PAD, 48.0, 16.0, rgba(p.text, 0.06));
        let (temp, sky) = st.weather.split_once("  ").unwrap_or((&st.weather, ""));
        big.draw(&mut cv, PAD + 16.0, y + 32.0, temp, ink);
        let tw = big.width(temp);
        let sky = clip(&mut mid, sky, W as f32 - 2.0 * PAD - 48.0 - tw);
        mid.draw(&mut cv, PAD + 16.0 + tw + 14.0, y + 29.0, &sky, dim);
        y += 58.0;
    }

    // calendar: this month, Monday first, today lit
    if let Some(c) = &st.cal {
        let cw = (W as f32 - 2.0 * PAD) / 7.0;
        let tw = mid.width(&c.title);
        mid.draw(&mut cv, (W as f32 - tw) / 2.0, y + 14.0, &c.title, ink);
        y += 28.0;
        for (i, d) in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].iter().enumerate() {
            let dw = small.width(d);
            small.draw(&mut cv, PAD + i as f32 * cw + (cw - dw) / 2.0, y + 12.0, d, dim);
        }
        y += 20.0;
        for n in 0..c.days {
            let slot = c.first + n;
            let (cx, cy) = (PAD + (slot % 7) as f32 * cw + cw / 2.0, y + (slot / 7) as f32 * 26.0 + 12.0);
            let label = (n + 1).to_string();
            if n + 1 == c.day {
                cv.round_rect(cx - 12.0, cy - 12.0, 24.0, 24.0, 12.0, gold);
            }
            let lw = small.width(&label);
            small.draw(&mut cv, cx - lw / 2.0, cy + 4.0, &label, if n + 1 == c.day { rgba(p.sky, 1.0) } else { ink });
        }
        y += ((c.first + c.days).div_ceil(7)) as f32 * 26.0 + 14.0;
    }

    // notifications
    mid.draw(&mut cv, PAD, y + 14.0, "notifications", ink);
    if !st.notes.is_empty() {
        let r = (W as f32 - PAD - 56.0, y - 2.0, 56.0, 24.0);
        let lit = hover == Some(Hit::Clear);
        cv.round_rect(r.0, r.1, r.2, r.3, 12.0, rgba(p.text, if lit { 0.18 } else { 0.07 }));
        let cw = small.width("clear");
        small.draw(&mut cv, r.0 + (r.2 - cw) / 2.0, r.1 + 16.0, "clear", dim);
        hits.push((r, Hit::Clear));
    }
    y += 34.0;
    let room = ((h as f32 - 16.0 - y) / NOTE_H).floor() as usize;
    if st.notes.is_empty() {
        cv.round_rect(PAD, y, W as f32 - 2.0 * PAD, 110.0, 20.0, rgba(p.text, 0.04));
        cv.icon_bell(W as f32 / 2.0, y + 44.0, 20.0, rgba(p.text, 0.35));
        let nw = small.width("nothing new");
        small.draw(&mut cv, (W as f32 - nw) / 2.0, y + 94.0, "nothing new", dim);
    }
    for n in st.notes.iter().take(room) {
        cv.round_rect(PAD, y, W as f32 - 2.0 * PAD, NOTE_H - 8.0, 14.0, rgba(p.text, 0.06));
        cv.star5(PAD + 16.0, y + 17.0, 5.0, gold);
        let when = ago(st.now.saturating_sub(n.at));
        let ww = small.width(&when);
        small.draw(&mut cv, W as f32 - PAD - 12.0 - ww, y + 20.0, &when, dim);
        let head = clip(&mut mid, &n.summary, W as f32 - 2.0 * PAD - 40.0 - 44.0);
        mid.draw(&mut cv, PAD + 30.0, y + 22.0, &head, ink);
        let body = clip(&mut small, &format!("{}  {}", n.app, n.body), W as f32 - 2.0 * PAD - 40.0);
        small.draw(&mut cv, PAD + 30.0, y + 40.0, &body, dim);
        y += NOTE_H;
    }
    if st.notes.len() > room && room > 0 {
        let more = format!("+{} more", st.notes.len() - room);
        small.draw(&mut cv, PAD, h as f32 - 14.0, &more, dim);
    }
    (cv, hits)
}

struct Panel {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    height: u32,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    exit: bool,
    entered: bool,
    st: State,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    drag: Option<Hit>,
}

impl Panel {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending || !self.dirty {
            return;
        }
        self.dirty = false;
        let (cv, hits) = render(&self.st, self.height, self.hover);
        self.hits = hits;
        let (buffer, canvas) = self.pool.create_buffer(cv.w as i32, cv.h as i32, cv.w as i32 * 4, wl_shm::Format::Argb8888).expect("create buffer");
        let bytes = unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.px.len() * 4) };
        canvas[..bytes.len()].copy_from_slice(bytes);
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, cv.w as i32, cv.h as i32);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        buffer.attach_to(surface).expect("buffer attach");
        self.layer.commit();
        self.frame_pending = true;
    }

    fn slide(&mut self, hit: Hit, x: f32) {
        match hit {
            Hit::Volume => self.st.set_volume(along(x)),
            Hit::Brightness => self.st.set_brightness(along(x)),
            _ => {}
        }
        self.dirty = true;
    }

    fn press(&mut self, hit: Option<Hit>, x: f32) {
        match hit {
            Some(Hit::Power) => {
                let _ = Command::new("seiza-session").spawn();
                self.exit = true;
            }
            Some(Hit::Mute) => {
                run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
                self.st.muted = !self.st.muted;
                self.dirty = true;
            }
            Some(h @ (Hit::Volume | Hit::Brightness)) => {
                self.drag = Some(h);
                self.slide(h, x);
            }
            Some(Hit::Tile(i)) => {
                if self.st.edit {
                    self.st.flip_hidden(i);
                } else if self.st.toggle(i) {
                    self.exit = true;
                }
                self.dirty = true;
            }
            Some(Hit::Edit) => {
                self.st.edit = !self.st.edit;
                self.dirty = true;
            }
            Some(Hit::Clear) => {
                let _ = std::fs::File::create(notes_path());
                self.st.notes.clear();
                self.dirty = true;
            }
            None => {}
        }
    }
}

impl CompositorHandler for Panel {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Panel {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Panel {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        let h = c.new_size.1.max(1);
        if self.configured && h == self.height {
            return; // Seiza re-sends the same configure after every commit
        }
        self.height = h;
        self.configured = true;
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for Panel {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Panel {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if event.keysym == Keysym::Escape {
            self.exit = true;
        }
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: RawModifiers, _: u32) {}
}

impl PointerHandler for Panel {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let (x, y) = (e.position.0 as f32, e.position.1 as f32);
            let hit = find(&self.hits, x, y);
            match e.kind {
                PointerEventKind::Enter { .. } => self.entered = true,
                PointerEventKind::Leave { .. } if self.entered && self.drag.is_none() => self.exit = true,
                PointerEventKind::Motion { .. } => {
                    if let Some(d) = self.drag {
                        self.slide(d, x);
                    } else if hit != self.hover {
                        self.hover = hit;
                        self.dirty = true;
                    }
                }
                PointerEventKind::Press { button: BTN_LEFT, .. } => self.press(hit, x),
                PointerEventKind::Release { button: BTN_LEFT, .. } => self.drag = None,
                _ => {}
            }
        }
        self.draw(qh);
    }
}

impl ShmHandler for Panel {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Panel);

impl ProvidesRegistryState for Panel {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Panel);

/// Toggle: a running panel is closed by running it again (`open` leaves it be); returns true if we should quit.
fn single_instance(open_only: bool) -> bool {
    let path = format!("{}/seiza-panel.pid", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()));
    if let Some(pid) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
        if std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "seiza-panel") && pid != std::process::id() {
            if !open_only {
                let _ = Command::new("kill").arg(pid.to_string()).status();
            }
            return true;
        }
    }
    let _ = std::fs::write(&path, std::process::id().to_string());
    false
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let st = State {
            vol: 0.42,
            muted: false,
            bright: Some(0.7),
            wifi: true,
            bt: false,
            mic_on: true,
            notes: parse_notes("1000\tnotify-send\tDownload finished\tdebian-13.iso\n1090\tseiza\tseiza.kdl has 1 mistake(s)\tline 12: unknown key zoom in the motion block"),
            now: 1100,
            weather: "12°C  Partly cloudy".into(),
            cal: Some(cal_of(2026, 11, 2, "November 2026".into())),
            hidden: vec![],
            edit: false,
        };
        let (cv, _) = render(&st, 960, None);
        let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
        ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
        std::fs::write(args.get(1).expect("--dump needs a file"), ppm).unwrap();
        return;
    }
    if single_instance(args.first().map(String::as_str) == Some("open")) {
        return;
    }
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-panel"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::RIGHT);
    layer.set_margin(56, 12, 12, 0);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(W, 0);
    layer.commit();

    let mut app = Panel {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        keyboard: None,
        pointer: None,
        height: 1,
        configured: false,
        frame_pending: false,
        dirty: true,
        exit: false,
        entered: false,
        st: State::read(),
        hits: vec![],
        hover: None,
        drag: None,
    };
    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
    let _ = std::fs::remove_file(format!("{}/seiza-panel.pid", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_parse_newest_first_and_skip_junk() {
        let n = parse_notes("10\ta\tfirst\tbody\nnot a line\n20\tb\tsecond\t\n");
        assert_eq!(n.iter().map(|n| n.summary.as_str()).collect::<Vec<_>>(), ["second", "first"]);
        assert_eq!(n[1].body, "body");
    }

    #[test]
    fn calendar_knows_the_weekday_and_leap_years() {
        let c = cal_of(2026, 11, 2, String::new());
        assert_eq!((c.first, c.days), (6, 30)); // 1 Nov 2026 is a Sunday: last column
        assert_eq!(cal_of(2024, 2, 1, String::new()).days, 29);
        assert_eq!(cal_of(2024, 2, 1, String::new()).first, 3); // Thursday
    }

    #[test]
    fn slider_maps_the_track_and_clamps() {
        assert_eq!(along(0.0), 0.0);
        assert_eq!(along(1000.0), 1.0);
        assert!((along(TRACK_X + TRACK_W / 2.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn hidden_tiles_leave_the_grid_until_editing() {
        let mut st = State { vol: 0.5, muted: false, bright: None, wifi: false, bt: false, mic_on: true, notes: vec![], now: 0, weather: String::new(), cal: None, hidden: vec!["rec".into()], edit: false };
        assert_eq!(st.shown().len(), TILES.len() - 1);
        st.edit = true;
        assert_eq!(st.shown().len(), TILES.len());
        assert_eq!(TILES.len() - 3, ACTIONS.len());
    }

    #[test]
    fn clicks_land_on_the_right_control() {
        let st = State { vol: 0.5, muted: false, bright: Some(0.5), wifi: false, bt: false, mic_on: true, notes: parse_notes("1\ta\ts\tb"), now: 5, weather: String::new(), cal: None, hidden: vec!["rec".into()], edit: false };
        let (_, hits) = render(&st, 960, None);
        let at = |h: Hit| hits.iter().find(|(_, x)| *x == h).map(|(r, _)| (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0)).unwrap();
        for h in [Hit::Power, Hit::Mute, Hit::Volume, Hit::Brightness, Hit::Tile(0), Hit::Tile(2), Hit::Tile(3), Hit::Clear, Hit::Edit] {
            let (x, y) = at(h);
            assert_eq!(find(&hits, x, y), Some(h));
        }
    }
}
