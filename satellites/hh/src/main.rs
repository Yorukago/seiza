//! seiza-hh: HueHaven's palettes in a Seiza window. Pick one and Enter paints Seiza with it
//! (the old palette file is kept as palette.bak); Tab saves Seiza's current palette into
//! HueHaven's Custom collection. HueHaven itself is not modified: its palette table is compiled
//! in by build.rs and its config files are read the way its README describes.
//!
//! SEIZA_HH_DRY=1 writes the palette file but restarts nothing.
//! `seiza-hh --dump out.ppm [query] [visibility]` draws one frame to a file.
mod catalog;
mod place;
mod ui;

use seiza_common::draw::Canvas;
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
use std::time::Instant;
use ui::Ui;

const OPEN_SECS: f32 = 0.35;
const CLOSE_SECS: f32 = 0.2;
const BTN_LEFT: u32 = 0x110;

fn pid_file() -> String {
    format!("{}/seiza-hh.pid", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()))
}

/// One window at a time: a second run closes the first (so the key toggles it).
fn claim_singleton() -> bool {
    let path = pid_file();
    if let Some(pid) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
        let ours = std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "seiza-hh");
        if ours && pid != std::process::id() {
            let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
            return false;
        }
    }
    let _ = std::fs::write(&path, std::process::id().to_string());
    true
}

struct Hh {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    exit: bool,
    chord: bool, // ctrl or alt held: not typing
    opened: Option<Instant>,
    closing: Option<Instant>,
    shown: f32,
    ui: Ui,
}

impl Hh {
    fn visibility(&mut self) -> (f32, bool) {
        let t = self.opened.map_or(0.0, |o| (o.elapsed().as_secs_f32() / OPEN_SECS).min(1.0));
        let open = 1.0 - (1.0 - t).powi(3);
        let Some(c) = self.closing else { return (open, t < 1.0) };
        let ct = (c.elapsed().as_secs_f32() / CLOSE_SECS).min(1.0);
        if ct >= 1.0 {
            self.exit = true;
        }
        (open.min((1.0 - ct).powi(2)), true)
    }

    fn close(&mut self) {
        self.closing.get_or_insert_with(Instant::now);
        self.dirty = true;
    }

    /// Paint Seiza with the selected palette.
    fn apply(&mut self) {
        let Some(colors) = self.ui.current().map(|p| p.colors) else { return };
        match place::apply(&place::place(&colors)) {
            Ok(()) => self.close(),
            Err(e) => self.ui.msg = Some(format!("couldn't write the palette: {e}")),
        }
        self.dirty = true;
    }

    /// Save Seiza's current palette as a Custom palette in HueHaven.
    fn save(&mut self) {
        let roles = place::Roles::parse(&std::fs::read_to_string(seiza_common::config::path("palette")).unwrap_or_default());
        let name = format!("Seiza {}", seiza_common::local_time(c"%Y-%m-%d %H:%M"));
        self.ui.msg = Some(match catalog::append(&name, "Saved from your Seiza sky.", &roles.colors()) {
            Ok(()) => {
                self.ui.reload();
                format!("saved as \"{name}\" in huehaven's custom palettes")
            }
            Err(e) => format!("couldn't save: {e}"),
        });
        self.dirty = true;
    }

    fn key(&mut self, event: &KeyEvent) {
        self.ui.msg = None;
        match event.keysym {
            Keysym::Escape => self.close(),
            Keysym::Return | Keysym::KP_Enter => self.apply(),
            Keysym::Tab => self.save(),
            Keysym::F2 => {
                self.ui.toggle_fav();
                self.dirty = true;
                return;
            }
            Keysym::BackSpace => {
                self.ui.query.pop();
                self.ui.refresh();
            }
            Keysym::Up => self.ui.step(-1),
            Keysym::Down => self.ui.step(1),
            Keysym::Page_Up => self.ui.step(-6),
            Keysym::Page_Down => self.ui.step(6),
            _ => match event.utf8.as_deref() {
                Some(s) if !self.chord && !s.is_empty() && !s.chars().any(char::is_control) => {
                    self.ui.query.push_str(s);
                    self.ui.refresh();
                }
                _ => return,
            },
        }
        self.dirty = true;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending {
            return;
        }
        let (e, moving) = self.visibility();
        if self.exit || !(self.dirty || moving || e != self.shown) {
            return;
        }
        self.dirty = false;
        self.shown = e;
        let cv = self.ui.render(e);
        let (buffer, canvas) = self.pool.create_buffer(cv.w as i32, cv.h as i32, cv.w as i32 * 4, wl_shm::Format::Argb8888).expect("create buffer");
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

impl CompositorHandler for Hh {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Hh {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Hh {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        let size = (c.new_size.0.max(1) as f32, c.new_size.1.max(1) as f32);
        // Seiza re-sends the same configure after every commit; redrawing for it would loop forever.
        if self.configured && size == (self.ui.w, self.ui.h) {
            return;
        }
        (self.ui.w, self.ui.h) = size;
        self.configured = true;
        self.opened.get_or_insert_with(Instant::now);
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for Hh {
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
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard
            && let Some(k) = self.keyboard.take() {
                k.release();
            }
        if cap == Capability::Pointer
            && let Some(p) = self.pointer.take() {
                p.release();
            }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Hh {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key(&event);
        self.draw(qh);
    }
    fn repeat_key(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if event.keysym != Keysym::Return && event.keysym != Keysym::Escape && event.keysym != Keysym::Tab {
            self.key(&event);
            self.draw(qh);
        }
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        self.chord = m.ctrl || m.alt || m.logo;
    }
}

impl PointerHandler for Hh {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let (x, y) = (e.position.0 as f32, e.position.1 as f32);
            let hit = self.ui.hit(x, y);
            match e.kind {
                PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                    if let Some(i) = hit.filter(|&i| i != self.ui.sel) {
                        self.ui.select(i);
                        self.dirty = true;
                    }
                }
                PointerEventKind::Press { button: BTN_LEFT, .. } => match hit {
                    Some(i) => {
                        self.ui.select(i);
                        self.apply();
                    }
                    None if self.ui.outside(x, y) => self.close(),
                    None => {}
                },
                PointerEventKind::Axis { vertical, .. } => {
                    self.ui.scroll_by(if vertical.absolute > 0.0 { 1 } else { -1 });
                    self.dirty = true;
                }
                _ => {}
            }
        }
        self.draw(qh);
    }
}

impl ShmHandler for Hh {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Hh);

impl ProvidesRegistryState for Hh {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Hh);

fn dump(path: &str, args: &[String]) {
    let mut ui = Ui::new(catalog::load(), 1920.0, 1080.0);
    ui.query = args.first().cloned().unwrap_or_default();
    ui.refresh();
    let cv: Canvas = ui.render(args.get(1).and_then(|e| e.parse().ok()).unwrap_or(1.0));
    let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
    ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
    std::fs::write(path, ppm).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        return dump(args.get(1).expect("--dump needs a file"), &args[2..]);
    }
    if !claim_singleton() {
        return;
    }

    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-hh"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0);
    layer.commit();

    let mut app = Hh {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        keyboard: None,
        pointer: None,
        configured: false,
        frame_pending: false,
        dirty: true,
        exit: false,
        chord: false,
        opened: None,
        closing: None,
        shown: 0.0,
        ui: Ui::new(catalog::load(), 1.0, 1.0),
    };
    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
    let _ = std::fs::remove_file(pid_file());
}
