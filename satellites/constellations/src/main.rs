//! seiza-constellations: draw your constellation. The middle of the window IS the sky:
//! click to add a star, drag one to move it, click two to join them, right-click to remove
//! one. Templates (built in, or saved from your sky) lay the stars out on a figure.
//! Everything goes to Seiza as commands, and the window follows what Seiza says.
//! `seiza-constellations --dump out.ppm` draws one frame to a file.
mod model;
mod ui;

use std::sync::{Arc, Mutex};

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
use model::{Sky, Star};
use ui::{Effect, Ui};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

struct Editor {
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
    chord: bool, // ctrl or alt held: not typing
    down: bool,  // a button held: dragging a star
    live: Arc<Mutex<Option<Sky>>>, // what Seiza last said about the sky
    seen: Option<Sky>,
    ui: Ui,
}

impl Editor {
    fn run(&mut self, effect: Effect) {
        if let Effect::Run(cmds) = effect {
            for c in cmds {
                match seiza_common::command(&c) {
                    Ok(reply) if reply.starts_with("error") => {
                        self.ui.msg = reply.replacen("error: ", "", 1);
                        break; // the rest depended on this one
                    }
                    Ok(_) => self.ui.msg = if c.starts_with("seiza figure") { "stars laid out. press alt+o to see them..".into() } else { String::new() },
                    Err(e) => {
                        self.ui.msg = format!("couldn't reach seiza: {e}");
                        break;
                    }
                }
            }
        }
        self.dirty = true;
    }

    fn key(&mut self, event: &KeyEvent) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        match event.keysym {
            Keysym::Escape if self.ui.focus.is_some() => self.ui.focus = None,
            Keysym::Escape if self.ui.selected.is_some() => self.ui.selected = None,
            Keysym::Escape => self.exit = true,
            Keysym::Return | Keysym::KP_Enter => {
                let e = self.ui.enter(w, h);
                self.run(e);
            }
            Keysym::BackSpace if self.ui.focus.is_some() => {
                self.ui.backspace();
            }
            Keysym::BackSpace | Keysym::Delete => {
                let e = self.ui.delete_selected();
                self.run(e);
            }
            _ => match event.utf8.as_deref() {
                Some(s) if self.ui.focus.is_some() && !self.chord && !s.chars().any(char::is_control) => self.ui.type_text(s),
                _ => return,
            },
        }
        self.dirty = true;
    }

    /// Pick up what Seiza last said about the stars.
    fn poll(&mut self) {
        let now = self.live.lock().ok().and_then(|l| l.clone());
        if now != self.seen {
            if let Some(sky) = now.clone() {
                self.ui.set_sky(sky);
                self.dirty = true;
            }
            self.seen = now;
        }
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending || !self.dirty {
            return;
        }
        self.dirty = false;
        let cv = self.ui.render(self.size.0 as usize, self.size.1 as usize);
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

impl CompositorHandler for Editor {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Editor {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Editor {
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
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for Editor {
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

impl KeyboardHandler for Editor {
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
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        self.chord = m.ctrl || m.alt;
    }
}

impl PointerHandler for Editor {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let (x, y) = (e.position.0 as f32, e.position.1 as f32);
            match e.kind {
                PointerEventKind::Press { button, .. } if button == BTN_LEFT || button == BTN_RIGHT => {
                    self.down = button == BTN_LEFT;
                    let effect = self.ui.press(x, y, w, h, button == BTN_RIGHT);
                    self.run(effect);
                }
                PointerEventKind::Motion { .. } if self.down => {
                    self.ui.drag_to(x, y, w, h);
                    self.dirty = true;
                }
                PointerEventKind::Release { button: BTN_LEFT, .. } => {
                    self.down = false;
                    let effect = self.ui.release();
                    self.run(effect);
                    self.dirty = true;
                }
                _ => {}
            }
        }
        self.draw(qh);
    }
}

impl ShmHandler for Editor {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// The stars are Seiza's, so a thread listens to its state stream; a sync callback a few
/// times a second wakes the event loop to pick up what it heard.
struct Tick;

impl Dispatch<wl_callback::WlCallback, Tick> for Editor {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Tick, _: &Connection, qh: &QueueHandle<Self>) {
        state.poll();
        state.draw(qh);
    }
}

delegate_registry!(Editor);

impl ProvidesRegistryState for Editor {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Editor);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let mut ui = Ui::new();
        let star = |name: &str, x: f64, y: f64, windows| Star { name: name.into(), x, y, windows };
        ui.set_sky(Sky {
            stars: vec![star("star 0", 960.0, 540.0, 3), star("north", 5760.0, 1300.0, 0), star("star 2", 9800.0, 540.0, 1), star("star 3", 7200.0, 4600.0, 0), star("star 4", 2400.0, 5200.0, 0)],
            links: vec![(0, 1), (1, 2), (1, 3), (3, 4)],
            current: "north".into(),
        });
        ui.selected = Some(1);
        ui.star_name = "north".into();
        let cv = ui.render(1920, 1080);
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

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-constellations"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0);
    layer.commit();

    let live: Arc<Mutex<Option<Sky>>> = Arc::new(Mutex::new(None));
    let listen = live.clone();
    std::thread::spawn(move || {
        seiza_common::subscribe(|st| {
            let stars = st.stars.iter().map(|s| Star { name: s.name.clone(), x: s.x, y: s.y, windows: s.windows }).collect();
            *listen.lock().unwrap() = Some(Sky { stars, links: st.links, current: st.current });
            true
        });
    });
    let (tick_conn, tick_qh) = (conn.clone(), qh.clone());
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(150));
        tick_conn.display().sync(&tick_qh, Tick);
        let _ = tick_conn.flush();
    });

    let mut app = Editor {
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
        chord: false,
        down: false,
        live,
        seen: None,
        ui: Ui::new(),
    };
    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
}
