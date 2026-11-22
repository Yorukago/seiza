//! seiza-widgets: a clock with the moon's phase, a calendar, the machine's pulse and what is playing,
//! as cards on the sky, under your windows. Which ones, and in what order, is
//! `~/.config/seiza/widgets`: one of clock / calendar / system / media / weather / agenda per line (weather and agenda are only shown when listed)
//! (an empty file shows nothing; no file shows all four). Drag a card to move it: it is then pinned
//! to the star you dropped it on and only shows there; cards never dragged sit in a row on the first star.
//! Right-click a card to send it to the next star. Pins are kept in `~/.config/seiza/widgetpos`; delete that file to put the cards back.
//! `seiza-widgets --dump out.ppm` draws one frame to a file.
mod feeds;
mod ui;

use std::{sync::mpsc::{channel, Receiver}, time::Duration};

use seiza_common::sys::{read_cpu_counters, read_load, read_ram_pct, read_temp};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::client::{
        globals::registry_queue_init,
        protocol::{wl_callback, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        Connection, Dispatch, QueueHandle,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use ui::{parse_media, Media, Pin, Pulse, Ui, MEDIA_FORMAT};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

fn config_cards() -> Vec<String> {
    let dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    match std::fs::read_to_string(format!("{dir}/seiza/widgets")) {
        Ok(text) => text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect(),
        Err(_) => ["clock", "calendar", "system", "media"].map(String::from).to_vec(),
    }
}

/// What the network and the calendar file have said last; the card threads write it, the tick reads it.
#[derive(Default)]
struct Feeds {
    weather: Option<feeds::Weather>,
    agenda: Option<Vec<(String, String)>>,
}

const FEED_EVERY: Duration = Duration::from_secs(30 * 60);

/// One thread per card that needs the outside world: fetch, wait half an hour, fetch again.
// shortcut: settings are read once at start, so a new place or calendar needs the widgets restarted.
fn start_feeds(cards: &[String], feeds: std::sync::Arc<std::sync::Mutex<Feeds>>) {
    if cards.iter().any(|c| c == "weather") {
        let feeds = feeds.clone();
        std::thread::spawn(move || {
            let place = seiza_common::config::get("weather", "place").unwrap_or_default();
            let imperial = seiza_common::config::get("weather", "units").as_deref() == Some("imperial");
            loop {
                if let Some(w) = feeds::fetch_weather(&place, imperial) {
                    feeds.lock().unwrap().weather = Some(w);
                }
                std::thread::sleep(FEED_EVERY);
            }
        });
    }
    if cards.iter().any(|c| c == "agenda") {
        std::thread::spawn(move || {
            let src = seiza_common::config::get("calendar", "source").unwrap_or_default();
            loop {
                if !src.trim().is_empty() {
                    if let Some(text) = feeds::read_calendar(&src) {
                        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
                        let rows = feeds::upcoming(feeds::parse_ics(&text), now, 4).iter().map(|e| (feeds::when_text(e), e.what.clone())).collect();
                        feeds.lock().unwrap().agenda = Some(rows);
                    }
                }
                std::thread::sleep(Duration::from_secs(5 * 60)); // a file is cheap; a calendar address is fetched again after this
            }
        });
    }
}

// shortcut: one playerctl spawn a second; switch to `playerctl --follow` if it ever shows in a profile.
fn read_media() -> Option<Media> {
    let out = std::process::Command::new("playerctl").args(["-a", "metadata", "--format", MEDIA_FORMAT]).output().ok()?;
    parse_media(&String::from_utf8_lossy(&out.stdout))
}

/// A card's pin from `widgetpos` ("clock=-300,120,default,work": dx,dy, constellation, star).
fn load_pins(ui: &mut Ui) {
    for name in ui.cards.clone() {
        let pin = seiza_common::config::get("widgetpos", &name).and_then(|v| {
            let mut f = v.splitn(4, ',');
            Some(Pin { dx: f.next()?.trim().parse().ok()?, dy: f.next()?.trim().parse().ok()?, constellation: f.next()?.to_string(), star: f.next()?.to_string() })
        });
        if let Some(pin) = pin {
            ui.pin.insert(name, pin);
        }
    }
}

struct Widgets {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor: CompositorState,
    pointer: Option<wl_pointer::WlPointer>,
    frame_pending: bool,
    dirty: bool,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    size: (u32, u32),
    configured: bool,
    shown: String,
    cpu_prev: Option<(u64, u64)>,
    states: Receiver<seiza_common::State>,
    feeds: std::sync::Arc<std::sync::Mutex<Feeds>>,
    ui: Ui,
}

impl Widgets {
    /// Draw now, or at the next frame callback if one is already on its way.
    fn request_draw(&mut self, qh: &QueueHandle<Self>) {
        self.dirty = true;
        self.draw(qh);
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending || !self.dirty {
            return;
        }
        self.dirty = false;
        let (w, h) = self.size;
        self.shown = self.ui.key();
        let cv = self.ui.render(w as usize, h as usize);
        let (buffer, canvas) = self.pool.create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888).expect("create buffer");
        // shortcut: assumes a little-endian host, which Argb8888 in memory then matches.
        let bytes = unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.px.len() * 4) };
        canvas[..bytes.len()].copy_from_slice(bytes);
        let surface = self.layer.wl_surface();
        // only the cards take clicks; the rest of the sky is whatever is behind them
        let region = Region::new(&self.compositor).expect("wl_region");
        for (x, y, cw, ch) in self.ui.rects(w as f32, h as f32) {
            region.add(x as i32, y as i32, cw as i32, ch as i32);
        }
        surface.set_input_region(Some(region.wl_region()));
        surface.damage_buffer(0, 0, w as i32, h as i32);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.frame_pending = true;
        buffer.attach_to(surface).expect("buffer attach");
        self.layer.commit();
    }

    /// The compositor says the camera or the stars changed: cards pinned to a star move with it.
    fn state_changed(&mut self, qh: &QueueHandle<Self>) {
        if let Some(st) = self.states.try_iter().last() {
            self.ui.set_view(&st);
            self.request_draw(qh);
        }
    }

    /// Once a second: read the machine, redraw if anything shows differently.
    fn tick(&mut self, qh: &QueueHandle<Self>) {
        let counters = read_cpu_counters();
        let cpu = match (self.cpu_prev, counters) {
            (Some((t0, i0)), Some((t1, i1))) if t1 > t0 => Some((100 * ((t1 - t0) - (i1 - i0)) / (t1 - t0)) as u32),
            _ => None,
        };
        self.cpu_prev = counters;
        self.ui.set_pulse(Pulse { cpu, ram: read_ram_pct(), temp: read_temp(), load: read_load() });
        if self.ui.cards.iter().any(|c| c == "media") {
            self.ui.set_media(read_media());
        }
        {
            let f = self.feeds.lock().unwrap();
            if f.weather != self.ui.weather || f.agenda != self.ui.agenda {
                (self.ui.weather, self.ui.agenda) = (f.weather.clone(), f.agenda.clone());
            }
        }
        if self.ui.key() != self.shown {
            self.request_draw(qh);
        }
    }
}

impl CompositorHandler for Widgets {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Widgets {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Widgets {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        std::process::exit(0);
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        let size = (c.new_size.0.max(1), c.new_size.1.max(1));
        // Seiza re-sends the same configure after every commit; redrawing for it would loop forever.
        if self.configured && size == self.size {
            return;
        }
        self.size = size;
        self.configured = true;
        self.request_draw(qh);
    }
}

impl ShmHandler for Widgets {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// The machine has no events to ride on, so a thread asks the compositor for a sync
/// callback every second; the reply wakes the event loop to read it.
struct Tick;

impl Dispatch<wl_callback::WlCallback, Tick> for Widgets {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Tick, _: &Connection, qh: &QueueHandle<Self>) {
        state.tick(qh);
    }
}

impl SeatHandler for Widgets {
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

impl PointerHandler for Widgets {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let (px, py) = (e.position.0 as f32, e.position.1 as f32);
            match e.kind {
                PointerEventKind::Press { button: BTN_LEFT, .. } => {
                    if self.ui.press(px, py, w, h) {
                        self.request_draw(qh); // it is raised above the others
                    }
                }
                PointerEventKind::Press { button: BTN_RIGHT, .. } => {
                    if let Some((name, pin)) = self.ui.send_next(px, py, w, h) {
                        let _ = seiza_common::config::set("widgetpos", &name, &format!("{:.0},{:.0},{},{}", pin.dx, pin.dy, pin.constellation, pin.star));
                        self.request_draw(qh);
                    }
                }
                PointerEventKind::Motion { .. } => {
                    if self.ui.motion(px, py) {
                        self.request_draw(qh);
                    }
                }
                PointerEventKind::Release { button: BTN_LEFT, .. } => {
                    if let Some(cmd) = self.ui.tap() {
                        std::thread::spawn(move || std::process::Command::new("playerctl").arg(cmd).status()); // the thread reaps it
                    }
                    if let Some((name, pin)) = self.ui.release(w, h) {
                        let _ = seiza_common::config::set("widgetpos", &name, &format!("{:.0},{:.0},{},{}", pin.dx, pin.dy, pin.constellation, pin.star));
                    }
                    self.request_draw(qh); // a card that could not be pinned goes back
                }
                _ => {}
            }
        }
    }
}

struct StateWake;

impl Dispatch<wl_callback::WlCallback, StateWake> for Widgets {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &StateWake, _: &Connection, qh: &QueueHandle<Self>) {
        state.state_changed(qh);
    }
}

delegate_registry!(Widgets);

impl ProvidesRegistryState for Widgets {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(Widgets);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let mut ui = Ui::new(config_cards());
        ui.set_pulse(Pulse { cpu: read_cpu_counters().map(|_| 23), ram: read_ram_pct(), temp: read_temp(), load: read_load() });
        ui.set_media(read_media());
        let feeds = std::sync::Arc::new(std::sync::Mutex::new(Feeds::default()));
        start_feeds(&ui.cards, feeds.clone());
        std::thread::sleep(Duration::from_secs(3)); // the dump waits a moment for the cards that fetch
        let f = feeds.lock().unwrap();
        (ui.weather, ui.agenda) = (f.weather.clone(), f.agenda.clone());
        drop(f);
        let cv = ui.render(1920, 760);
        let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
        ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
        std::fs::write(args.get(1).expect("--dump needs a file"), ppm).unwrap();
        return;
    }
    let app_cards = config_cards();
    let mut ui = Ui::new(app_cards.clone());
    load_pins(&mut ui);
    if ui.width() <= 1 {
        return; // no widgets asked for: nothing to show
    }
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    // The surface is the whole screen so cards can be dragged anywhere; its input region (set on
    // every draw) is only the cards, so the rest of the sky is whatever is behind them.
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Bottom, Some("seiza-widgets"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, 0);
    layer.commit();

    let (tick_conn, tick_qh) = (conn.clone(), qh.clone());
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        tick_conn.display().sync(&tick_qh, Tick);
        let _ = tick_conn.flush();
    });

    let (tx, states) = channel();
    let (state_conn, state_qh) = (conn.clone(), qh.clone());
    std::thread::spawn(move || {
        seiza_common::subscribe(|st| {
            // the reply to this sync wakes the event loop, which then reads the state from the channel
            let sent = tx.send(st).is_ok();
            state_conn.display().sync(&state_qh, StateWake);
            let _ = state_conn.flush();
            sent
        })
    });

    let feeds = std::sync::Arc::new(std::sync::Mutex::new(Feeds::default()));
    start_feeds(&app_cards, feeds.clone());
    let mut app = Widgets {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        compositor,
        pointer: None,
        frame_pending: false,
        dirty: false,
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        size: (1, 1),
        configured: false,
        shown: String::new(),
        cpu_prev: read_cpu_counters(),
        states,
        feeds,
        ui,
    };
    loop {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn no_file_means_all_three_cards() {
        let dir = std::env::temp_dir().join(format!("seiza-widgets-test-{}", std::process::id()));
        // SAFETY: tests in this crate do not run in parallel with other readers of this variable.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        assert_eq!(super::config_cards(), ["clock", "calendar", "system", "media"]);
        std::fs::create_dir_all(dir.join("seiza")).unwrap();
        std::fs::write(dir.join("seiza/widgets"), "# mine\ncalendar\n\nclock\n").unwrap();
        assert_eq!(super::config_cards(), ["calendar", "clock"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
