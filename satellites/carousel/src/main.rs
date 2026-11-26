//! seiza-carousel: the constellation view. A film strip with a card for each constellation,
//! the one you are in in the middle; click a star (or press Enter) to go into it, or make a
//! new constellation on the last card. Left and right (or the arrows, or the wheel) look around.
//! `seiza-carousel --dump out.ppm [demo]` draws one frame to a file.
mod ui;

use std::time::Instant;

use seiza_common::{
    constellation::{self, Constellation},
    draw::Canvas,
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
use ui::{Effect, Ui};

const BTN_LEFT: u32 = 0x110;
const OPEN_SECS: f32 = 0.35;
const CLOSE_SECS: f32 = 0.2;

/// One carousel at a time: a second run closes the first (so the key toggles it).
/// Returns false when it closed a running one and this process should just exit.
fn claim_singleton() -> bool {
    let path = format!("{}/seiza-carousel.pid", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()));
    if let Some(pid) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
        let ours = std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "seiza-carousel");
        if ours && pid != std::process::id() {
            let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
            return false;
        }
    }
    let _ = std::fs::write(&path, std::process::id().to_string());
    true
}

struct Carousel {
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
    opened: Option<Instant>,
    closing: Option<Instant>,
    shown: f32, // the visibility last drawn
    ui: Ui,
}

impl Carousel {
    /// Visibility now, 0..1, and whether it is still moving.
    fn visibility(&mut self) -> (f32, bool) {
        let t = self.opened.map_or(0.0, |o| (o.elapsed().as_secs_f32() / OPEN_SECS).min(1.0));
        let open = 1.0 - (1.0 - t).powi(3); // eases out: fast, then settles
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

    /// Send what the interface decided to Seiza; the first command that fails stops the rest.
    fn run(&mut self, effect: Effect) {
        let cmds = match effect {
            Effect::None => return,
            Effect::Open { name, star } => {
                let mut c = vec![];
                if name != self.ui.current {
                    c.push(format!("seiza load {name}"));
                }
                c.push(format!("seiza goto {}", star + 1));
                c
            }
            Effect::Create(name) => vec![format!("seiza new-constellation {name}"), "seiza goto 1".into()],
        };
        self.dirty = true;
        for c in cmds {
            if std::env::var_os("SEIZA_CAROUSEL_DRY").is_some() {
                println!("would send: {c}");
                continue;
            }
            match seiza_common::command(&c) {
                Ok(reply) if reply.starts_with("error") => {
                    self.ui.msg = reply.replacen("error: ", "", 1);
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    self.ui.msg = format!("couldn't reach seiza: {e}");
                    return;
                }
            }
        }
        self.close();
    }

    fn key(&mut self, event: &KeyEvent) {
        self.ui.msg.clear();
        match event.keysym {
            Keysym::Escape => self.close(),
            Keysym::Left => self.ui.step(-1),
            Keysym::Right => self.ui.step(1),
            Keysym::Home => self.ui.select(0),
            Keysym::End => self.ui.select(usize::MAX),
            Keysym::BackSpace => self.ui.backspace(),
            Keysym::Return | Keysym::KP_Enter => {
                let e = self.ui.activate();
                self.run(e);
            }
            _ => match event.utf8.as_deref() {
                Some(s) if !self.chord && !s.is_empty() && !s.chars().any(char::is_control) => self.ui.type_text(s),
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
        let moving = moving || self.ui.sliding();
        if self.exit || !(self.dirty || moving || e != self.shown) {
            return;
        }
        self.dirty = false;
        self.shown = e;
        let cv = self.ui.render(self.size.0 as usize, self.size.1 as usize, e);
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

impl CompositorHandler for Carousel {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Carousel {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Carousel {
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
        self.opened.get_or_insert_with(Instant::now);
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for Carousel {
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

impl KeyboardHandler for Carousel {
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
        self.chord = m.ctrl || m.alt || m.logo;
    }
}

impl PointerHandler for Carousel {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            match e.kind {
                PointerEventKind::Press { button: BTN_LEFT, .. } => {
                    let effect = self.ui.click(e.position.0 as f32, e.position.1 as f32, w, h);
                    self.run(effect);
                    self.dirty = true;
                }
                PointerEventKind::Axis { vertical, .. } if vertical.absolute != 0.0 => {
                    self.ui.step(if vertical.absolute > 0.0 { 1 } else { -1 });
                    self.dirty = true;
                }
                _ => {}
            }
        }
        self.draw(qh);
    }
}

impl ShmHandler for Carousel {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Carousel);

impl ProvidesRegistryState for Carousel {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Carousel);

/// The constellations on disk, with the open one taken as it is right now from Seiza (its file may be
/// a moment behind); and which one, and which star, you are in.
fn load() -> (Vec<Constellation>, String, String) {
    let mut cards: Vec<Constellation> = constellation::list().iter().filter_map(|n| constellation::load(n)).collect();
    let Some(st) = seiza_common::snapshot() else { return (cards, String::new(), String::new()) };
    let live = Constellation { name: st.constellation.clone(), stars: st.stars.iter().map(|s| (s.name.clone(), s.x, s.y)).collect(), links: st.links.clone() };
    match cards.iter_mut().find(|c| c.name == live.name) {
        Some(c) => *c = live,
        None => cards.push(live),
    }
    (cards, st.constellation, st.current)
}

/// Made-up constellations, for `--dump demo`: a crown and a dipper, laid out the way the compositor does.
fn demo() -> (Vec<Constellation>, String, String) {
    let shape = |name: &str, pts: &[(f64, f64)], links: &[(usize, usize)], scale: f64| Constellation {
        name: name.into(),
        stars: pts.iter().enumerate().map(|(i, p)| (format!("star {i}"), 960.0 + p.0 * scale, 540.0 + p.1 * scale)).collect(),
        links: links.to_vec(),
    };
    let dipper = shape("working", &[(0.0, 0.15), (0.2, 0.38), (0.4, 0.52), (0.6, 0.45), (0.64, 0.85), (1.0, 0.92), (1.0, 0.52)], &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 3)], 12000.0);
    let crown = shape("chilling", &[(0.0, 0.7), (0.14, 0.28), (0.4, 0.04), (0.68, 0.04), (0.92, 0.3), (1.0, 0.74)], &[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)], 12000.0);
    (vec![dipper, crown], "working".into(), "star 3".into())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let (cards, current, star) = if args.get(2).map(String::as_str) == Some("demo") { demo() } else { load() };
        let mut ui = Ui::new(cards, current, star);
        if let Some(i) = args.get(3).and_then(|i| i.parse().ok()) {
            ui.select(i);
        }
        let cv: Canvas = ui.render(1920, 1080, 1.0);
        let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
        ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
        std::fs::write(args.get(1).expect("--dump needs a file"), ppm).unwrap();
        return;
    }
    if !claim_singleton() {
        return;
    }
    let (cards, current, star) = load();

    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-carousel"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0);
    layer.commit();

    let mut app = Carousel {
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
        opened: None,
        closing: None,
        shown: 0.0,
        ui: Ui::new(cards, current, star),
    };

    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
    let _ = std::fs::remove_file(format!("{}/seiza-carousel.pid", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())));
}
