//! seiza-help: the key bindings (live), the commands, the words Seiza uses and its config
//! files. `seiza-help --dump out.ppm [page [width height [visibility 0..1 [search]]]]` draws one frame to a file.
mod pages;
mod ui;

use std::time::SystemTime;

use seiza_common::draw::Canvas;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::client::{
        globals::registry_queue_init,
        protocol::{wl_callback, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        Connection, Dispatch, QueueHandle,
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
use ui::Ui;

const BTN_LEFT: u32 = 0x110;

struct Help {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    size: (u32, u32),
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    exit: bool,
    keys_seen: Option<SystemTime>, // the keys file as last loaded
    anim: seiza_common::kit::Anim,
    shown: f32, // the visibility last drawn
    ui: Ui,
}

impl Help {
    /// Visibility now, 0..1, and whether it is still moving. The card rises while it opens and drops while it closes.
    fn visibility(&mut self) -> (f32, bool) {
        let (e, moving, done) = self.anim.visibility();
        self.exit |= done;
        (e, moving)
    }

    fn close(&mut self) {
        self.anim.close();
        self.dirty = true;
    }

    fn key(&mut self, event: &KeyEvent) {
        match event.keysym {
            Keysym::Escape => {
                if !self.ui.clear_search() {
                    self.close();
                }
            }
            Keysym::BackSpace => self.ui.backspace(),
            Keysym::Left if !self.ui.searching() => self.ui.go(self.ui.page.saturating_sub(1)),
            Keysym::Right | Keysym::Tab if !self.ui.searching() => self.ui.go(self.ui.page + 1),
            Keysym::Up => self.ui.scroll_by(-1),
            Keysym::Down => self.ui.scroll_by(1),
            Keysym::Page_Up => self.ui.scroll_by(-8),
            Keysym::Page_Down => self.ui.scroll_by(8),
            _ => match event.utf8.as_deref() {
                Some(s) if !s.chars().any(char::is_control) => self.ui.type_str(s),
                _ => return,
            },
        }
        self.dirty = true;
    }

    /// If the keys file changed, have Seiza re-read it (which rewrites the dump), then show the new dump.
    fn refresh_keys(&mut self) {
        let edited = std::fs::metadata(seiza_common::config::path("keys")).and_then(|m| m.modified()).ok();
        if edited != self.keys_seen {
            self.keys_seen = edited;
            let _ = seiza_common::command("seiza reload-config");
        }
        self.ui.reload_keys();
        self.dirty = true;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending {
            return;
        }
        let (e, moving) = self.visibility();
        if self.exit || !(self.dirty || moving || self.ui.pending() || e != self.shown) {
            return;
        }
        self.dirty = false;
        self.shown = e;
        let cv: Canvas = self.ui.render(self.size.0 as usize, self.size.1 as usize, e);
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

impl CompositorHandler for Help {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Help {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Help {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        let size = (c.new_size.0.max(1), c.new_size.1.max(1));
        // Seiza re-sends the same configure after every commit; redrawing for it would loop forever.
        if self.configured && size == self.size {
            return;
        }
        self.size = size;
        self.configured = true;
        self.anim.open();
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for Help {
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
        if cap == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
        if cap == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Help {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key(&event);
        self.draw(qh);
    }
    fn repeat_key(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if event.keysym != Keysym::Return && event.keysym != Keysym::Escape {
            self.key(&event);
            self.draw(qh);
        }
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: RawModifiers, _: u32) {
    }
}

impl PointerHandler for Help {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            match e.kind {
                PointerEventKind::Press { button: BTN_LEFT, .. } => {
                    self.dirty |= self.ui.press(e.position.0 as f32, e.position.1 as f32, self.size.0 as f32, self.size.1 as f32);
                }
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.dirty |= self.ui.hover(Some((e.position.0 as f32, e.position.1 as f32)), self.size.0 as f32, self.size.1 as f32);
                }
                PointerEventKind::Leave { .. } => self.dirty |= self.ui.hover(None, self.size.0 as f32, self.size.1 as f32),
                PointerEventKind::Axis { vertical, .. } if vertical.absolute != 0.0 => {
                    self.ui.scroll_by(if vertical.absolute > 0.0 { 1 } else { -1 });
                    self.dirty = true;
                }
                _ => {}
            }
        }
        self.draw(qh);
    }
}

impl ShmHandler for Help {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// The keys file can change while this is open, so a thread asks the compositor for a sync
/// callback twice a second; its reply wakes the event loop, which checks the file.
struct Tick;

impl Dispatch<wl_callback::WlCallback, Tick> for Help {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Tick, _: &Connection, qh: &QueueHandle<Self>) {
        let seen = state.keys_seen;
        let edited = std::fs::metadata(seiza_common::config::path("keys")).and_then(|m| m.modified()).ok();
        if edited != seen {
            state.refresh_keys();
            state.draw(qh);
        }
        if state.ui.tick() {
            state.dirty = true; // a page that moves by itself (the distance to Voyager)
            state.draw(qh);
        }
    }
}

delegate_registry!(Help);

impl ProvidesRegistryState for Help {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Help);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let mut ui = Ui::new();
        ui.page = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(0).min(ui.page_count() - 1);
        let size = |i: usize, d: usize| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
        if let Some(q) = args.get(6) {
            ui.type_str(q);
        }
        let cv = ui.render(size(3, 1920), size(4, 1080), args.get(5).and_then(|v| v.parse().ok()).unwrap_or(1.0));
        let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
        ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
        std::fs::write(args.get(1).expect("--dump needs a file"), ppm).unwrap();
        return;
    }
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-help"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0);
    layer.commit();

    let (tick_conn, tick_qh) = (conn.clone(), qh.clone());
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        tick_conn.display().sync(&tick_qh, Tick);
        let _ = tick_conn.flush();
    });

    let mut app = Help {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        keyboard: None,
        pointer: None,
        size: (1, 1),
        configured: false,
        frame_pending: false,
        dirty: true,
        exit: false,
        keys_seen: std::fs::metadata(seiza_common::config::path("keys")).and_then(|m| m.modified()).ok(),
        anim: Default::default(),
        shown: -1.0,
        ui: Ui::new(),
    };
    while !app.exit {
        if event_queue.blocking_dispatch(&mut app).is_err() {
            break; // the compositor went away
        }
    }
}
