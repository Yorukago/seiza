//! seiza-bar: a top bar for Seiza. Shows the constellation, one pill per star
//! (click or Alt+1..9 to fly there; dots count its windows) and the clock.
mod tray;

use std::{process::Command, thread, time::{Duration, Instant}};

use seiza_common::{
    local_time,
    sys::{read_cpu_counters, read_load, read_ram_pct, read_temp},
    palette::{palette, rgba},
    draw::{load_font, Canvas, Rgba, Text},
    State, STAR_COLORS,
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

const BAR_H: u32 = 32;
const HIDDEN_H: u32 = 3; // autohide: the strip that still feels the pointer
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

// ---- battery, volume, PC status ---------------------------------------------

/// (percent, charging) of the first battery, if the machine has one.
fn read_battery() -> Option<(u32, bool)> {
    let dir = std::fs::read_dir("/sys/class/power_supply").ok()?.filter_map(Result::ok).find(|e| e.file_name().to_string_lossy().starts_with("BAT"))?.path();
    let pct = std::fs::read_to_string(dir.join("capacity")).ok()?.trim().parse().ok()?;
    let status = std::fs::read_to_string(dir.join("status")).unwrap_or_default();
    Some((pct, status.trim() == "Charging"))
}

/// (percent, muted) of the default output, via PipeWire's wpctl.
fn read_volume() -> Option<(u32, bool)> {
    let out = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?; // "Volume: 0.67" or "Volume: 0.67 [MUTED]"
    let v: f32 = text.split_whitespace().nth(1)?.parse().ok()?;
    Some(((v * 100.0).round() as u32, text.contains("MUTED")))
}

/// Run a wpctl subcommand without making the bar wait for it.
fn wpctl(args: &'static [&'static str]) {
    thread::spawn(move || {
        let _ = Command::new("wpctl").args(args).status();
    });
}

// ---- the app ----------------------------------------------------------------

/// Which modules of the bar are shown: `date`, `stats`, `volume`, `battery` in ~/.config/seiza/bar, each on unless switched off.
struct Modules {
    date: bool,
    stats: bool,
    volume: bool,
    battery: bool,
    autohide: bool,
}

impl Modules {
    fn load() -> Modules {
        let on = |k| seiza_common::config::flag("bar", k, true);
        Modules { date: on("date"), stats: on("stats"), volume: on("volume"), battery: on("battery"), autohide: seiza_common::config::flag("bar", "autohide", false) }
    }
}

struct Bar {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
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
    hits: Vec<(f32, f32, usize)>,
    battery: Option<(u32, bool)>,
    volume: Option<(u32, bool)>,
    last_poll: Option<Instant>,
    vol_hit: (f32, f32), // x range of the volume widget: scroll changes it, click mutes
    mode_hit: (f32, f32), // the mode icon: opens the constellation view
    session_hit: (f32, f32), // the power button: opens the session menu
    tray: Option<tray::Tray>,
    tray_hits: Vec<(f32, f32)>, // x range of each tray icon
    cpu: Option<u32>,
    cpu_prev: Option<(u64, u64)>,
    show: Modules, // which modules the settings page switched on
    ram: Option<u32>,
    temp: Option<u32>,
    load: Option<f32>,
}

impl Bar {
    /// A rounded capsule behind one group of widgets.
    fn module(cv: &mut Canvas, x: f32, w: f32) {
        cv.round_rect(x, 4.0, w, BAR_H as f32 - 8.0, (BAR_H as f32 - 8.0) / 2.0, rgba(palette().dust, 0.55));
    }

    fn render(&mut self) -> Canvas {
        let (w, h) = (self.width as usize, BAR_H as usize);
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        cv.round_rect(0.0, 0.0, w as f32, h as f32, h as f32 / 2.0, rgba(palette().panel, 0.78)); // the floating pill
        self.hits.clear();
        let base = h as f32 / 2.0 + FONT_PX * 0.33;
        let cy = h as f32 / 2.0;
        let ink: Rgba = rgba(palette().text, 1.0);
        let dim: Rgba = rgba(palette().text, 0.65);
        let accent: Rgba = rgba(palette().star, 1.0);

        let Some(st) = self.state.clone() else {
            self.text.draw(&mut cv, 16.0, base, "waiting for seiza...", dim);
            return cv;
        };

        // left: mode icon (the constellation: crescent for "chill", star otherwise), date, time
        let mut x = 6.0;
        Self::module(&mut cv, x, 30.0);
        if st.constellation == "chill" {
            cv.crescent(x + 15.0, cy, 8.0, accent);
        } else {
            cv.star5(x + 15.0, cy, 9.0, accent);
        }
        self.mode_hit = (x, x + 30.0);
        x += 30.0 + 6.0;
        let (date, time) = (if self.show.date { date_text() } else { String::new() }, time_text());
        let (dw, tw) = (self.text.width(&date), self.text.width(&time));
        if date.is_empty() {
            Self::module(&mut cv, x, tw + 28.0);
            self.text.draw(&mut cv, x + 14.0, base, &time, ink);
        } else {
            Self::module(&mut cv, x, dw + tw + 46.0);
            self.text.draw(&mut cv, x + 14.0, base, &date, ink);
            cv.round_rect(x + 14.0 + dw + 8.0, cy - 6.0, 1.0, 12.0, 0.5, dim);
            self.text.draw(&mut cv, x + 14.0 + dw + 18.0, base, &time, ink);
        }

        // center: one sparkle per open window in the current star
        let idx = st.stars.iter().position(|s| s.name == st.current).unwrap_or(0);
        let col = STAR_COLORS[idx % 6];
        let n = st.stars.get(idx).map_or(0, |s| s.windows.min(8)) as f32;
        let span = n * 22.0 + 12.0;
        if n > 0.0 {
            let x0 = (w as f32 - span) / 2.0;
            Self::module(&mut cv, x0, span);
            for i in 0..n as usize {
                cv.sparkle(x0 + 17.0 + i as f32 * 22.0, cy, 7.5, [col[0], col[1], col[2], 1.0]);
            }
        }

        // right, from the edge inward: PC status, then volume and battery
        let mut rx = w as f32 - 8.0 - 30.0;
        Self::module(&mut cv, rx, 30.0);
        cv.icon_power(rx + 15.0, cy, 7.5, [1.0, 0.55, 0.62, 1.0]);
        self.session_hit = (rx, rx + 30.0);
        rx -= 6.0;
        self.tray_hits.clear();
        let icons = self.tray.as_ref().map(|t| t.items.lock().unwrap().clone()).unwrap_or_default();
        if !icons.is_empty() {
            let tw = icons.len() as f32 * (tray::ICON as f32 + 6.0) + 12.0;
            rx -= tw;
            Self::module(&mut cv, rx, tw);
            for (i, it) in icons.iter().enumerate() {
                let ix = rx + 9.0 + i as f32 * (tray::ICON as f32 + 6.0);
                self.tray_hits.push((ix - 3.0, ix + tray::ICON as f32 + 3.0));
                if it.px.is_empty() { // no icon found: a disc with the app's first letter
                    cv.round_rect(ix, cy - 9.0, 18.0, 18.0, 9.0, rgba(palette().text, 0.3));
                    let letter = it.label.chars().next().unwrap_or('?').to_uppercase().to_string();
                    let lw = self.text.width(&letter);
                    self.text.draw(&mut cv, ix + (18.0 - lw) / 2.0, base, &letter, ink);
                    continue;
                }
                for (j, &p) in it.px.iter().enumerate() { // premultiplied source-over
                    let (x, y) = (ix as usize + j % tray::ICON, (cy - 9.0) as usize + j / tray::ICON);
                    let d = &mut cv.px[y * cv.w + x];
                    let inv = 255 - (p >> 24);
                    let ch = |s: u32| ((p >> s & 255) + (*d >> s & 255) * inv / 255).min(255) << s;
                    *d = ch(24) | ch(16) | ch(8) | ch(0);
                }
            }
            rx -= 6.0;
        }
        let mut stats = Vec::new();
        if self.show.stats {
            if let Some(c) = self.cpu { stats.push(format!("cpu {c}%")); }
            if let Some(r) = self.ram { stats.push(format!("ram {r}%")); }
            if let Some(t) = self.temp { stats.push(format!("{t}°C")); }
            if let Some(l) = self.load { stats.push(format!("load {l:.2}")); }
        }
        let widths: Vec<f32> = stats.iter().map(|t| self.text.width(t)).collect();
        let sw: f32 = widths.iter().sum::<f32>() + 14.0 * stats.len().saturating_sub(1) as f32 + 28.0;
        if !stats.is_empty() {
            rx -= sw;
            Self::module(&mut cv, rx, sw);
            let mut tx = rx + 14.0;
            for (t, tw) in stats.iter().zip(&widths) {
                self.text.draw(&mut cv, tx, base, t, dim);
                tx += tw + 14.0;
            }
            rx -= 6.0;
        }
        if let Some((pct, charging)) = self.battery.filter(|_| self.show.battery) {
            let label = format!("{pct}%");
            let bw = 24.0 + 8.0 + self.text.width(&label) + 24.0;
            rx -= bw;
            Self::module(&mut cv, rx, bw);
            let fill: Rgba = if charging { [0.45, 0.90, 0.95, 1.0] } else if pct <= 15 { [1.0, 0.35, 0.40, 1.0] } else if pct <= 30 { [1.0, 0.80, 0.40, 1.0] } else { [0.55, 0.95, 0.70, 1.0] };
            let bx = rx + 12.0;
            cv.round_rect(bx, cy - 6.0, 22.0, 12.0, 3.0, rgba(palette().text, 0.55)); // shell
            cv.round_rect(bx + 1.5, cy - 4.5, 19.0, 9.0, 2.0, rgba(palette().panel, 1.0));
            cv.round_rect(bx + 22.0, cy - 2.5, 2.0, 5.0, 1.0, rgba(palette().text, 0.55)); // nub
            cv.round_rect(bx + 2.5, cy - 3.5, 17.0 * pct as f32 / 100.0, 7.0, 1.5, fill);
            self.text.draw(&mut cv, bx + 24.0 + 8.0, base, &label, ink);
            rx -= 6.0;
        }
        self.vol_hit = (0.0, 0.0);
        if let Some((pct, muted)) = self.volume.filter(|_| self.show.volume) {
            let label = if muted { "muted".to_string() } else { format!("{pct}%") };
            let vw = 18.0 + 8.0 + self.text.width(&label) + 24.0;
            rx -= vw;
            Self::module(&mut cv, rx, vw);
            for i in 0..4 {
                let lit = !muted && (pct as f32) > i as f32 * 25.0;
                let bh = 4.0 + 3.0 * i as f32;
                cv.round_rect(rx + 12.0 + i as f32 * 4.5, cy + 6.0 - bh, 3.0, bh, 1.0, if lit { accent } else { rgba(palette().text, 0.25) });
            }
            self.text.draw(&mut cv, rx + 12.0 + 18.0 + 8.0, base, &label, if muted { dim } else { ink });
            self.vol_hit = (rx, rx + vw);
        }
        cv
    }

    /// Autohide: shrink to the strip at the top edge, or grow back to the pill. The buffer follows the next configure.
    fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        self.left_at = None;
        self.layer.set_size(0, if hidden { HIDDEN_H } else { BAR_H });
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
            let (battery, volume) = (read_battery(), read_volume());
            let counters = read_cpu_counters();
            let cpu = match (self.cpu_prev, counters) {
                (Some((t0, i0)), Some((t1, i1))) if t1 > t0 => Some((100.0 * (1.0 - (i1 - i0) as f64 / (t1 - t0) as f64)).round() as u32),
                _ => self.cpu,
            };
            self.cpu_prev = counters;
            let (ram, temp, load) = (read_ram_pct(), read_temp(), read_load());
            let load = load.map(|l| (l * 100.0).round() / 100.0);
            if (battery, volume, cpu, ram, temp, load) != (self.battery, self.volume, self.cpu, self.ram, self.temp, self.load) {
                (self.battery, self.volume, self.cpu, self.ram, self.temp, self.load) = (battery, volume, cpu, ram, temp, load);
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
        let cv = if self.hidden { Canvas { w: self.width as usize, h: HIDDEN_H as usize, px: vec![0; self.width as usize * HIDDEN_H as usize] } } else { self.render() };
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
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            match e.kind {
                PointerEventKind::Enter { .. } if self.hidden => self.set_hidden(false),
                PointerEventKind::Enter { .. } => self.left_at = None,
                PointerEventKind::Leave { .. } => self.left_at = Some(Instant::now()),
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
            match e.kind {
                PointerEventKind::Press { button: BTN_LEFT, .. } if on_volume => {
                    wpctl(&["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
                    self.last_poll = None; // re-read now, not in 2s
                }
                PointerEventKind::Press { button: BTN_LEFT, .. } if x >= self.session_hit.0 && x < self.session_hit.1 => {
                    let _ = Command::new("seiza-session").spawn(); // the power menu is its own program
                }
                PointerEventKind::Press { button: BTN_LEFT, .. } if x >= self.mode_hit.0 && x < self.mode_hit.1 => {
                    let _ = Command::new("seiza-carousel").spawn(); // the constellation view is its own program (pressing again closes it)
                }
                PointerEventKind::Axis { vertical, .. } if on_volume && vertical.absolute != 0.0 => {
                    // scrolling up (negative) raises it; -l 1.0 stops it at 100%
                    wpctl(if vertical.absolute < 0.0 { &["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", "5%+"] } else { &["set-volume", "@DEFAULT_AUDIO_SINK@", "5%-"] });
                    self.last_poll = None;
                }
                _ => {}
            }
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
    layer.set_margin(8, 12, 0, 12); // a floating pill, not an edge-to-edge strip
    let autohide = seiza_common::config::flag("bar", "autohide", false);
    layer.set_exclusive_zone(if autohide { 0 } else { BAR_H as i32 + 8 }); // a hiding bar floats over the windows
    layer.set_size(0, if autohide { HIDDEN_H } else { BAR_H });
    if autohide {
        layer.set_margin(0, 12, 0, 12);
    }
    layer.commit();

    let mut app = Bar {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
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
        text: Text::new(load_font(), FONT_PX),
        hits: Vec::new(),
        battery: None,
        volume: None,
        last_poll: None,
        vol_hit: (0.0, 0.0),
        mode_hit: (0.0, 0.0),
        session_hit: (0.0, 0.0),
        tray: None,
        tray_hits: Vec::new(),
        cpu: None,
        cpu_prev: None,
        show: Modules::load(),
        ram: None,
        temp: None,
        load: None,
    };

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
