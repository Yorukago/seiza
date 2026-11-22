//! seiza-bar: a top bar for Seiza, as three floating islands: the mode star, the clock and what is playing
//! on the left; one star per star of the sky in the middle (click or Super+1..9 to fly there; the bright one
//! is where you are; pointing at one names it); the PC status, sound, battery and network on the right.
mod tray;

use std::{process::Command, sync::{Arc, Mutex, mpsc}, thread, time::{Duration, Instant}};

use seiza_common::{
    local_time,
    sys::{read_battery, read_cpu_counters, read_load, read_ram_pct, read_temp},
    palette::{palette, rgba},
    draw::{shared_font, Canvas, Rgba, Text},
    State,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            channel,
            timer::{TimeoutAction, Timer},
            EventLoop,
        },
        calloop_wayland_source::WaylandSource,
        client::{
            globals::registry_queue_init,
            protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
            Connection, QueueHandle,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

const ISLAND_H: u32 = 38;
const TIP_H: u32 = 30; // room under the islands for the name of the star the pointer is on
const SURF_H: u32 = ISLAND_H + TIP_H;
const HIDDEN_H: u32 = 6; // autohide: the strip that still feels the pointer
const STAR_PITCH: f32 = 34.0;
const HIDE_AFTER: Duration = Duration::from_millis(600);
const FONT_PX: f32 = 15.0;
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;
const TICK: Duration = Duration::from_millis(250);

fn date_text() -> String {
    local_time(c"%a %d %b")
}

fn time_text() -> String {
    local_time(seiza_common::clock_fmt())
}

// ---- volume, PC status ------------------------------------------------------

/// (percent, muted) of the default output, via PipeWire's wpctl.
fn read_volume(target: &str) -> Option<(u32, bool)> {
    let out = Command::new("wpctl").args(["get-volume", target]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?; // "Volume: 0.67" or "Volume: 0.67 [MUTED]"
    let v: f32 = text.split_whitespace().nth(1)?.parse().ok()?;
    Some(((v * 100.0).round() as u32, text.contains("MUTED")))
}

/// What a click on a part of the bar does.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Act {
    Goto(usize), // fly to star N (0-based)
    Media,       // play / pause
    Shot,
    Mic,
    Panel,
}

/// "Playing\tartist - title" from playerctl, if some player is open. Paused players still show, dimmed.
fn read_media() -> Option<(bool, String)> {
    let out = Command::new("playerctl").args(["metadata", "--format", "{{status}}\t{{artist}} - {{title}}"]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let (status, what) = text.trim().split_once('\t')?;
    let what = what.trim_start_matches(" - ");
    (!what.is_empty() && status != "Stopped").then(|| (status == "Playing", what.to_string()))
}

/// "eth", "wifi" or "off": the first connected device, from `nmcli`.
fn read_net() -> &'static str {
    let out = Command::new("nmcli").args(["-t", "-f", "TYPE,STATE", "device"]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    if out.contains("ethernet:connected") { "eth" } else if out.contains("wifi:connected") { "wifi" } else { "off" }
}

fn read_bt() -> bool {
    Command::new("bluetoothctl").arg("show").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("Powered: yes"))
}

/// What the bar learns by running programs. They take a few ms each, so a worker thread reads them
/// and the bar only looks at the latest result; run on the bar's own thread they stalled its animations every poll.
#[derive(Clone)]
struct Polled {
    volume: Option<(u32, bool)>,
    mic: Option<(u32, bool)>,
    media: Option<(bool, String)>,
    net: &'static str,
    bt: bool,
}

/// Reads every 2 s, or soon after a poke (something changed the volume); returns the poke end.
fn spawn_poller(cell: Arc<Mutex<Option<Polled>>>) -> mpsc::Sender<()> {
    let (poke, poked) = mpsc::channel::<()>();
    thread::spawn(move || loop {
        let p = Polled { volume: read_volume("@DEFAULT_AUDIO_SINK@"), mic: read_volume("@DEFAULT_AUDIO_SOURCE@"), media: read_media(), net: read_net(), bt: read_bt() };
        *cell.lock().unwrap() = Some(p);
        match poked.recv_timeout(Duration::from_secs(2)) {
            Ok(()) => thread::sleep(Duration::from_millis(80)), // let the wpctl that caused the poke finish
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    });
    poke
}

/// Cut `s` to fit `max` pixels, with an ellipsis.
fn clip(t: &mut Text, s: &str, max: f32) -> String {
    let mut out = s.to_string();
    if t.width(&out) <= max {
        return out;
    }
    while !out.is_empty() && t.width(&out) + 10.0 > max {
        out.pop();
    }
    out.trim_end().to_string() + "…"
}

/// Run a wpctl subcommand without making the bar wait for it.
fn wpctl(args: &'static [&'static str]) {
    thread::spawn(move || {
        let _ = Command::new("wpctl").args(args).status();
    });
}

/// Where a star's name goes: (x, width) of its pill, centred under the star but kept on the bar.
fn tip_box(star_x: f32, text_w: f32, bar_w: f32) -> (f32, f32) {
    let w = text_w + 24.0;
    ((star_x - w / 2.0).clamp(0.0, (bar_w - w).max(0.0)), w)
}

// ---- the app ----------------------------------------------------------------

/// Which modules of the bar are shown: `date`, `stats`, `volume`, `battery` in ~/.config/seiza/bar, each on unless switched off.
struct Modules {
    date: bool,
    stats: bool,
    volume: bool,
    battery: bool,
    autohide: bool,
    dots: bool,    // one dot per star in the middle
    media: bool,   // now playing
    quick: bool,   // screenshot and microphone buttons
    net: bool,     // network and bluetooth
    weather: bool, // the temperature (asks wttr.in, so off unless you turn it on)
}

impl Modules {
    fn load() -> Modules {
        let on = |k| seiza_common::config::flag("bar", k, true);
        Modules { date: on("date"), stats: on("stats"), volume: on("volume"), battery: on("battery"), autohide: seiza_common::config::flag("bar", "autohide", false), dots: on("dots"), media: on("media"), quick: on("quick"), net: on("net"), weather: seiza_common::config::flag("bar", "weather", false) }
    }
}

struct Bar {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    compositor: CompositorState,
    pointer: Option<wl_pointer::WlPointer>,
    width: u32,
    configured: bool,
    hidden: bool,                // autohide: only a thin strip at the top edge is left
    left_at: Option<Instant>,    // when the pointer left the shown bar
    frame_pending: bool,
    dirty: bool,
    exit: bool,
    state: Option<State>,
    clock: String,
    text: Text,
    hits: Vec<(f32, f32, Act)>, // x range of each clickable part
    battery: Option<(u32, bool)>,
    volume: Option<(u32, bool)>,
    last_poll: Option<Instant>,
    polled: Arc<Mutex<Option<Polled>>>, // the worker's latest reading
    poke: mpsc::Sender<()>,             // read again now, not in 2 s
    vol_hit: (f32, f32), // x range of the volume widget: scroll changes it, click mutes
    mode_hit: (f32, f32), // the mode icon: opens the constellation view
    hover_star: Option<usize>, // the star the pointer is on, to name it
    star_x: Vec<f32>,          // where each star is drawn
    tray: Option<tray::Tray>,
    tray_hits: Vec<(f32, f32)>, // x range of each tray icon
    cpu: Option<u32>,
    cpu_prev: Option<(u64, u64)>,
    show: Modules, // which modules the settings page switched on
    ram: Option<u32>,
    temp: Option<u32>,
    load: Option<f32>,
    media: Option<(bool, String)>,
    mic: Option<(u32, bool)>,
    net: &'static str,
    bt: bool,
    weather: std::sync::Arc<std::sync::Mutex<String>>,
}

impl Bar {
    /// A short vertical line between two groups inside an island.
    fn divider(cv: &mut Canvas, x: f32) {
        cv.round_rect(x, (ISLAND_H as f32 - 16.0) / 2.0, 1.0, 16.0, 0.5, rgba(palette().text, 0.16));
    }

    /// An island: a rounded capsule with a hairline edge, `w` wide from `x`.
    fn island(cv: &mut Canvas, x: f32, w: f32) {
        let (p, h) = (palette(), ISLAND_H as f32);
        cv.round_rect(x, 0.0, w, h, h / 2.0, rgba(p.text, 0.11));
        cv.round_rect(x + 1.0, 1.0, w - 2.0, h - 2.0, h / 2.0 - 1.0, rgba(p.panel, 0.88));
    }

    /// A clickable stretch of an island: remember it as one hit.
    fn hit(&mut self, x: f32, w: f32, a: Act) {
        self.hits.push((x, x + w, a));
    }

    fn render(&mut self) -> Canvas {
        let (w, h) = (self.width as usize, SURF_H as usize);
        let blank = || Canvas { w, h, px: vec![0; w * h] };
        let mut cv = blank();
        self.hits.clear();
        let cy = ISLAND_H as f32 / 2.0;
        let base = cy + FONT_PX * 0.33;
        let p = palette();
        let ink: Rgba = rgba(p.text, 1.0);
        let dim: Rgba = rgba(p.text, 0.62);
        let accent: Rgba = rgba(p.star, 1.0);

        let Some(st) = self.state.clone() else {
            let tw = self.text.width("waiting for seiza...");
            Self::island(&mut cv, 0.0, tw + 32.0);
            self.text.draw(&mut cv, 16.0, base, "waiting for seiza...", dim);
            return cv;
        };

        // left island: the mode star, the date and time, what is playing
        let mut lc = blank();
        let mut x = 14.0;
        if st.constellation == "chill" {
            lc.crescent(x + 8.0, cy, 8.0, rgba(p.pink, 1.0));
        } else {
            lc.sparkle(x + 8.0, cy, 9.5, rgba(p.pink, 1.0));
        }
        x += 16.0 + 12.0;
        self.mode_hit = (0.0, x + 6.0);
        Self::divider(&mut lc, x);
        x += 13.0;
        let (date, time) = (if self.show.date { date_text() } else { String::new() }, time_text());
        if !date.is_empty() {
            x = self.text.draw(&mut lc, x, base, &date, ink) + 9.0;
        }
        x = self.text.draw(&mut lc, x, base, &time, if date.is_empty() { ink } else { dim });
        if let Some((playing, what)) = self.media.clone().filter(|_| self.show.media) {
            x += 12.0;
            Self::divider(&mut lc, x);
            let start = x;
            x += 13.0;
            let what = clip(&mut self.text, &what, 240.0);
            lc.icon_note(x + 7.0, cy, 7.5, if playing { accent } else { dim });
            x = self.text.draw(&mut lc, x + 20.0, base, &what, if playing { ink } else { dim });
            self.hit(start, x + 14.0 - start, Act::Media);
        }
        Self::island(&mut cv, 0.0, x + 14.0);
        cv.over(&lc, 0, 0);

        // middle island: one star per star of the sky; the bright one is where you are
        self.star_x.clear();
        if self.show.dots && !st.stars.is_empty() {
            let n = st.stars.len().min(12);
            let span = n as f32 * STAR_PITCH + 12.0;
            let x0 = (w as f32 - span) / 2.0;
            Self::island(&mut cv, x0, span);
            let cur = st.stars.iter().position(|s| s.name == st.current).unwrap_or(0);
            for (i, s) in st.stars.iter().take(n).enumerate() {
                let sx = x0 + 6.0 + STAR_PITCH * (i as f32 + 0.5);
                self.star_x.push(sx);
                let lift = if self.hover_star == Some(i) { 1.0 } else { 0.0 };
                if i == cur {
                    cv.round_rect(sx - 17.0, cy - 17.0, 34.0, 34.0, 17.0, rgba(p.star, 0.06));
                    cv.round_rect(sx - 12.0, cy - 12.0, 24.0, 24.0, 12.0, rgba(p.star, 0.10));
                    cv.sparkle(sx, cy, 11.5, rgba(p.text, 1.0));
                } else {
                    let halo = 0.03 + 0.012 * ((i * 5) % 4) as f32; // each one a slightly different halo
                    cv.round_rect(sx - 9.0, cy - 9.0, 18.0, 18.0, 9.0, rgba(p.star, halo + 0.03 * lift));
                    cv.sparkle(sx, cy, 5.5 + 1.5 * lift, rgba(p.text, if lift > 0.0 { 0.95 } else if s.windows > 0 { 0.8 } else { 0.55 }));
                }
                self.hit(sx - STAR_PITCH / 2.0, STAR_PITCH, Act::Goto(i));
            }
        }

        // right island, from the edge inward: tray, network, quick buttons, battery, sound, weather, status
        let mut rc = blank();
        let mut rx = w as f32 - 14.0;
        let mut any = false;
        let seg = |rx: &mut f32, any: &mut bool, rc: &mut Canvas, sw: f32| -> f32 {
            if *any {
                *rx -= 12.0;
                Self::divider(rc, *rx);
                *rx -= 13.0;
            }
            *any = true;
            *rx -= sw;
            *rx
        };
        self.tray_hits.clear();
        let icons = self.tray.as_ref().map(|t| t.items.lock().unwrap().clone()).unwrap_or_default();
        if !icons.is_empty() {
            let tw = icons.len() as f32 * (tray::ICON as f32 + 6.0) - 6.0;
            let x = seg(&mut rx, &mut any, &mut rc, tw);
            for (i, it) in icons.iter().enumerate() {
                let ix = x + i as f32 * (tray::ICON as f32 + 6.0);
                self.tray_hits.push((ix - 3.0, ix + tray::ICON as f32 + 3.0));
                if it.px.is_empty() { // no icon found: a disc with the app's first letter
                    rc.round_rect(ix, cy - 9.0, 18.0, 18.0, 9.0, rgba(p.text, 0.3));
                    let letter = it.label.chars().next().unwrap_or('?').to_uppercase().to_string();
                    let lw = self.text.width(&letter);
                    self.text.draw(&mut rc, ix + (18.0 - lw) / 2.0, base, &letter, ink);
                    continue;
                }
                for (j, &px) in it.px.iter().enumerate() { // premultiplied source-over
                    let (px_x, px_y) = (ix as usize + j % tray::ICON, (cy - 9.0) as usize + j / tray::ICON);
                    let d = &mut rc.px[px_y * rc.w + px_x];
                    let inv = 255 - (px >> 24);
                    let ch = |s: u32| ((px >> s & 255) + (*d >> s & 255) * inv / 255).min(255) << s;
                    *d = ch(24) | ch(16) | ch(8) | ch(0);
                }
            }
        }
        if self.show.net {
            let x = seg(&mut rx, &mut any, &mut rc, 40.0);
            let off = rgba(p.text, 0.3);
            match self.net {
                "eth" => rc.icon_ethernet(x + 8.0, cy, 8.0, ink),
                "wifi" => rc.icon_wifi(x + 8.0, cy - 1.0, 7.0, ink),
                _ => rc.icon_wifi(x + 8.0, cy - 1.0, 7.0, off),
            }
            rc.icon_bluetooth(x + 32.0, cy, 8.0, if self.bt { accent } else { off });
            self.hit(x - 12.0, 64.0, Act::Panel);
        }
        if self.show.quick {
            let x = seg(&mut rx, &mut any, &mut rc, 42.0);
            rc.icon_frame(x + 8.0, cy, 7.5, ink);
            self.hit(x - 12.0, 32.0, Act::Shot);
            let muted = self.mic.is_none_or(|(_, m)| m);
            rc.icon_mic(x + 34.0, cy, 7.5, muted, if muted { rgba(p.text, 0.35) } else { accent });
            self.hit(x + 20.0, 34.0, Act::Mic);
        }
        if let Some((pct, charging)) = self.battery.filter(|_| self.show.battery) {
            let label = format!("{pct}%");
            let x = seg(&mut rx, &mut any, &mut rc, 28.0 + 8.0 + self.text.width(&label));
            let fill: Rgba = if charging { rgba(p.ice, 1.0) } else if pct <= 15 { rgba(p.pink, 1.0) } else { accent };
            rc.round_rect(x, cy - 6.0, 22.0, 12.0, 3.0, rgba(p.text, 0.6)); // shell
            rc.round_rect(x + 1.5, cy - 4.5, 19.0, 9.0, 2.0, rgba(p.panel, 1.0));
            rc.round_rect(x + 22.0, cy - 2.5, 2.0, 5.0, 1.0, rgba(p.text, 0.6)); // nub
            rc.round_rect(x + 2.5, cy - 3.5, (17.0 * pct as f32 / 100.0).max(1.0), 7.0, 1.5, fill);
            self.text.draw(&mut rc, x + 32.0, base, &label, ink);
        }
        self.vol_hit = (0.0, 0.0);
        if let Some((pct, muted)) = self.volume.filter(|_| self.show.volume) {
            let label = if muted { "muted".to_string() } else { format!("{pct}") };
            let vw = 18.0 + 8.0 + self.text.width(&label);
            let x = seg(&mut rx, &mut any, &mut rc, vw);
            rc.icon_speaker(x + 7.0, cy, 8.5, pct as f32 / 100.0 * 3.0, muted, if muted { rgba(p.text, 0.4) } else { ink });
            self.text.draw(&mut rc, x + 26.0, base, &label, if muted { dim } else { ink });
            self.vol_hit = (x - 12.0, x + vw + 12.0);
        }
        let temp = self.weather.lock().unwrap().clone();
        if self.show.weather && !temp.is_empty() {
            let x = seg(&mut rx, &mut any, &mut rc, self.text.width(&temp));
            self.text.draw(&mut rc, x, base, &temp, ink);
        }
        let mut stats = Vec::new();
        if self.show.stats {
            if let Some(c) = self.cpu { stats.push((format!("cpu {c}%"), ink)); }
            if let Some(r) = self.ram { stats.push((format!("ram {r}%"), ink)); }
            if let Some(t) = self.temp { stats.push((format!("{t}°C"), ink)); }
            if let Some(l) = self.load { stats.push((format!("load {l:.2}"), dim)); }
        }
        if !stats.is_empty() {
            let widths: Vec<f32> = stats.iter().map(|(t, _)| self.text.width(t)).collect();
            let sw = widths.iter().sum::<f32>() + 12.0 * (stats.len() - 1) as f32;
            let mut tx = seg(&mut rx, &mut any, &mut rc, sw);
            for ((t, c), tw) in stats.iter().zip(&widths) {
                self.text.draw(&mut rc, tx, base, t, *c);
                tx += tw + 12.0;
            }
        }
        if any {
            let left = (rx - 14.0).max(0.0);
            Self::island(&mut cv, left, w as f32 - left);
            cv.over(&rc, 0, 0);
        }

        // the name of the star the pointer is on, floating under it
        if let Some(i) = self.hover_star.filter(|&i| i < st.stars.len() && i < self.star_x.len()) {
            let label = format!("star {} \u{b7} {}", i + 1, st.stars[i].name);
            let (tx, tw) = tip_box(self.star_x[i], self.text.width(&label), w as f32);
            let ty = ISLAND_H as f32 + 4.0;
            cv.round_rect(tx, ty, tw, 24.0, 12.0, rgba(p.text, 0.12));
            cv.round_rect(tx + 1.0, ty + 1.0, tw - 2.0, 22.0, 11.0, rgba(p.panel, 0.92));
            self.text.draw(&mut cv, tx + 12.0, ty + 17.0, &label, ink);
        }
        cv
    }

    /// Autohide's strip: a short line at the top edge until the pointer touches it.
    fn strip(&self) -> Canvas {
        let (w, h) = (self.width as usize, HIDDEN_H as usize);
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        let p = palette();
        let x = (w as f32 - 440.0).max(0.0) / 2.0;
        cv.round_rect(x, 0.0, 440.0_f32.min(w as f32), 5.0, 2.5, rgba(p.text, 0.18));
        cv.round_rect(x + 1.0, 1.0, 440.0_f32.min(w as f32) - 2.0, 3.0, 1.5, rgba(p.panel, 0.95));
        cv
    }

    /// The pointer is on star `i` (or none): redraw if that changed.
    fn set_hover(&mut self, i: Option<usize>) {
        if self.hover_star != i {
            self.hover_star = i;
            self.dirty = true;
        }
    }

    /// Autohide: shrink to the strip at the top edge, or grow back to the pill. The buffer follows the next configure.
    fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        self.left_at = None;
        self.layer.set_size(0, if hidden { HIDDEN_H } else { SURF_H });
        self.layer.set_margin(if hidden { 0 } else { 8 }, 12, 0, 12);
        self.layer.commit();
        self.dirty = true;
    }

    fn tick(&mut self, qh: &QueueHandle<Self>) {
        let clock = format!("{}{}", date_text(), time_text());
        if clock != self.clock {
            self.clock = clock;
            self.dirty = true;
        }
        if self.last_poll.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.last_poll = Some(Instant::now());
            let battery = read_battery();
            let counters = read_cpu_counters();
            let cpu = match (self.cpu_prev, counters) {
                (Some((t0, i0)), Some((t1, i1))) if t1 > t0 => Some((100.0 * (1.0 - (i1 - i0) as f64 / (t1 - t0) as f64)).round() as u32),
                _ => self.cpu,
            };
            self.cpu_prev = counters;
            let (ram, temp, load) = (read_ram_pct(), read_temp(), read_load());
            let load = load.map(|l| (l * 100.0).round() / 100.0);
            if (battery, cpu, ram, temp, load) != (self.battery, self.cpu, self.ram, self.temp, self.load) {
                (self.battery, self.cpu, self.ram, self.temp, self.load) = (battery, cpu, ram, temp, load);
                self.dirty = true;
            }
        }
        let polled = self.polled.lock().unwrap().clone();
        if let Some(p) = polled {
            if (&p.media, p.mic, p.net, p.bt, p.volume) != (&self.media, self.mic, self.net, self.bt, self.volume) {
                (self.media, self.mic, self.net, self.bt, self.volume) = (p.media, p.mic, p.net, p.bt, p.volume);
                self.dirty = true;
            }
        }
        if self.show.autohide && !self.hidden && self.left_at.is_some_and(|t| t.elapsed() > HIDE_AFTER) {
            self.set_hidden(true);
        }
        if !self.configured || self.frame_pending || !self.dirty {
            return;
        }
        self.dirty = false;
        let cv = if self.hidden { self.strip() } else { self.render() };
        if let (Ok(path), Some(_)) = (std::env::var("SEIZA_BAR_DUMP"), &self.state) { // debugging aid: write the bar as PPM and quit
            let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
            ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
            std::fs::write(path, ppm).unwrap();
            self.exit = true;
            return;
        }
        let (buffer, canvas) = self
            .pool
            .create_buffer(cv.w as i32, cv.h as i32, cv.w as i32 * 4, wl_shm::Format::Argb8888)
            .expect("create buffer");
        // shortcut: assumes a little-endian host, which Argb8888 in memory then matches.
        let bytes = unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.px.len() * 4) };
        canvas[..bytes.len()].copy_from_slice(bytes);
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, cv.w as i32, cv.h as i32);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        buffer.attach_to(surface).expect("buffer attach");
        self.layer.commit();
        self.frame_pending = true;
    }
}

impl CompositorHandler for Bar {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Bar {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Bar {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        self.width = c.new_size.0.max(1);
        self.configured = true;
        self.dirty = true;
        // only the islands take the pointer: what floats under them (a star's name) lets clicks through
        if !self.hidden {
            if let Ok(region) = smithay_client_toolkit::compositor::Region::new(&self.compositor) {
                region.add(0, 0, self.width as i32, ISLAND_H as i32);
                self.layer.wl_surface().set_input_region(Some(region.wl_region()));
            }
        }
    }
}

impl SeatHandler for Bar {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Bar {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            match e.kind {
                PointerEventKind::Enter { .. } if self.hidden => self.set_hidden(false),
                PointerEventKind::Enter { .. } => self.left_at = None,
                PointerEventKind::Leave { .. } => {
                    self.left_at = Some(Instant::now());
                    self.set_hover(None);
                }
                PointerEventKind::Motion { .. } => {
                    let x = e.position.0 as f32;
                    let on = self.hits.iter().find_map(|&(a, b, act)| match act {
                        Act::Goto(i) if x >= a && x < b => Some(i),
                        _ => None,
                    });
                    self.set_hover(on);
                }
                _ => {}
            }
            let x = e.position.0 as f32;
            let on_volume = x >= self.vol_hit.0 && x < self.vol_hit.1;
            if let PointerEventKind::Press { button, .. } = e.kind {
                let method = match button { BTN_LEFT => "Activate", BTN_RIGHT => "ContextMenu", BTN_MIDDLE => "SecondaryActivate", _ => "" };
                if let (Some(i), Some(t)) = (self.tray_hits.iter().position(|&(a, b)| x >= a && x < b), &self.tray) {
                    if !method.is_empty() {
                        t.click(i, method);
                    }
                    continue;
                }
            }
            if let PointerEventKind::Press { button: BTN_LEFT, .. } = e.kind {
                if let Some(&(_, _, act)) = self.hits.iter().find(|&&(a, b, _)| x >= a && x < b) {
                    match act {
                        Act::Goto(i) => drop(seiza_common::command(&format!("seiza goto {}", i + 1))),
                        Act::Media => drop(Command::new("playerctl").arg("play-pause").spawn()),
                        Act::Shot => drop(Command::new("seiza-shot").arg("area").spawn()),
                        Act::Mic => {
                            wpctl(&["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]);
                            let _ = self.poke.send(());
                        }
                        Act::Panel => drop(Command::new("seiza-panel").spawn()),
                    }
                    continue;
                }
            }
            match e.kind {
                PointerEventKind::Press { button: BTN_LEFT, .. } if on_volume => {
                    wpctl(&["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
                    let _ = self.poke.send(());
                }
                PointerEventKind::Press { button: BTN_LEFT, .. } if x >= self.mode_hit.0 && x < self.mode_hit.1 => {
                    let _ = Command::new("seiza-carousel").spawn(); // the constellation view is its own program (pressing again closes it)
                }
                PointerEventKind::Axis { vertical, .. } if on_volume && vertical.absolute != 0.0 => {
                    // scrolling up (negative) raises it; -l 1.0 stops it at 100%
                    wpctl(if vertical.absolute < 0.0 { &["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "5%+"] } else { &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"] });
                    let _ = self.poke.send(());
                }
                _ => {}
            }
        }
        if self.dirty {
            self.tick(qh); // draw now, not at the next tick
        }
    }
}

impl ShmHandler for Bar {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Bar);

impl ProvidesRegistryState for Bar {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Bar);

fn main() {
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Top, Some("seiza-bar"), None);
    layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
    layer.set_margin(8, 12, 0, 12); // islands floating off the edges, not an edge-to-edge strip
    let autohide = seiza_common::config::flag("bar", "autohide", false);
    layer.set_exclusive_zone(if autohide { 0 } else { ISLAND_H as i32 + 8 }); // a hiding bar floats over the windows; the room for a star's name is not reserved
    layer.set_size(0, if autohide { HIDDEN_H } else { SURF_H });
    if autohide {
        layer.set_margin(0, 12, 0, 12);
    }
    layer.commit();

    let polled = Arc::new(Mutex::new(None));
    let poke = spawn_poller(polled.clone());
    let mut app = Bar {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        compositor,
        pointer: None,
        width: 1,
        configured: false,
        hidden: autohide,
        left_at: None,
        frame_pending: false,
        dirty: true,
        exit: false,
        state: None,
        clock: String::new(),
        text: Text::new(shared_font(), FONT_PX),
        hits: Vec::new(),
        battery: None,
        volume: None,
        last_poll: None,
        polled,
        poke,
        vol_hit: (0.0, 0.0),
        mode_hit: (0.0, 0.0),
        hover_star: None,
        star_x: Vec::new(),
        tray: None,
        tray_hits: Vec::new(),
        cpu: None,
        cpu_prev: None,
        show: Modules::load(),
        ram: None,
        temp: None,
        load: None,
        media: None,
        mic: None,
        net: "off",
        bt: false,
        weather: Default::default(),
    };
    if app.show.weather {
        let (cell, place) = (app.weather.clone(), seiza_common::config::get("weather", "place").unwrap_or_default());
        let units = if seiza_common::config::get("weather", "units").as_deref() == Some("imperial") { "u" } else { "m" };
        let place: String = place.trim().chars().map(|c| if c.is_ascii_alphanumeric() || "-_,.".contains(c) { c } else { '+' }).collect();
        thread::spawn(move || loop {
            let url = format!("https://wttr.in/{place}?{units}&format=%25t");
            if let Ok(o) = Command::new("curl").args(["-sf", "-m", "10", &url]).output() {
                let t = String::from_utf8_lossy(&o.stdout).trim().replace('+', "");
                if o.status.success() && !t.is_empty() && t.len() < 12 { // an error page is long
                    *cell.lock().unwrap() = t;
                }
            }
            thread::sleep(Duration::from_secs(1200));
        });
    }

    let mut event_loop: EventLoop<Bar> = EventLoop::try_new().unwrap();
    let handle = event_loop.handle();
    WaylandSource::new(conn, event_queue).insert(handle.clone()).unwrap();

    let (tx, rx) = channel::channel();
    thread::spawn(move || seiza_common::subscribe(|st| tx.send(st).is_ok()));
    handle
        .insert_source(rx, |ev, _, app: &mut Bar| {
            if let channel::Event::Msg(st) = ev {
                app.state = Some(st);
                app.dirty = true;
            }
        })
        .unwrap();
    handle
        .insert_source(Timer::from_duration(TICK), move |_, _, app| {
            app.tick(&qh);
            TimeoutAction::ToDuration(TICK)
        })
        .unwrap();

    let (wake, wake_rx) = channel::channel();
    app.tray = tray::Tray::start(wake);
    handle.insert_source(wake_rx, |_, _, app: &mut Bar| app.dirty = true).unwrap();

    let signal = event_loop.get_signal();
    event_loop
        .run(None, &mut app, |app| {
            if app.exit {
                signal.stop();
            }
        })
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stars_name_stays_on_the_bar() {
        assert_eq!(tip_box(500.0, 76.0, 1000.0), (450.0, 100.0), "centred under the star");
        assert_eq!(tip_box(10.0, 76.0, 1000.0).0, 0.0, "not off the left edge");
        assert_eq!(tip_box(995.0, 76.0, 1000.0).0, 900.0, "not off the right edge");
    }
}
