//! wonder: the Seiza music player. `seiza-wonder` opens its window; the other forms are for trying the parts
//! without one: `search <words>`, `lyrics <words>`, `serve [--silent] [words]` (the player alone, on the bus) and
//! `--dump out.ppm [page [width height [visibility 0..1]]]` (one frame of the window to a file).
mod cache;
mod cover;
mod history;
mod ipc;
mod lyrics;
mod mpris;
mod playlists;
mod player;
mod search;
mod ui;

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
use std::{rc::Rc, sync::mpsc::Receiver};
use ui::{Mini, Ui};

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;

/// What the window asks for when it ends.
#[derive(Clone, Copy, PartialEq)]
enum Next {
    Full,
    Mini,
    Hide,
    Gone, // the compositor went away
}

struct Wonder {
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
    ctrl: bool,
    mini: bool,
    next: Next,
    raised: Rc<Receiver<()>>,
    anim: seiza_common::kit::Anim,
    shown: f32, // the visibility last drawn
    ui: Ui,
}

impl Wonder {
    fn visibility(&mut self) -> (f32, bool) {
        let (e, moving, done) = self.anim.visibility();
        self.exit |= done;
        (e, moving)
    }

    fn close(&mut self) {
        if self.mini {
            self.exit = true; // the pill has no fade
        } else {
            self.anim.close();
        }
        self.dirty = true;
    }

    fn key(&mut self, event: &KeyEvent) {
        match event.keysym {
            Keysym::v | Keysym::V if self.ctrl => self.ui.paste(),
            Keysym::m | Keysym::M if self.ctrl => {
                self.next = Next::Mini;
                self.close();
            }
            Keysym::Escape => {
                if !self.ui.clear_query() {
                    self.close();
                }
            }
            Keysym::BackSpace => self.ui.backspace(),
            Keysym::Return | Keysym::KP_Enter => self.ui.enter(),
            Keysym::Delete if !self.ui.searching() => self.ui.delete_selected(),
            Keysym::s | Keysym::S if self.ctrl => self.ui.save_queue(),
            Keysym::Tab => self.ui.next_page(),
            Keysym::Up => self.ui.move_sel(-1),
            Keysym::Down => self.ui.move_sel(1),
            Keysym::Left if !self.ui.searching() => self.ui.step(-1),
            Keysym::Right if !self.ui.searching() => self.ui.step(1),
            Keysym::space if !self.ui.searching() => self.ui.toggle(),
            _ => match event.utf8.as_deref() {
                Some(s) if !s.chars().any(char::is_control) => self.ui.type_str(s),
                _ => return,
            },
        }
        self.dirty = true;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending {
            return;
        }
        let (e, moving) = if self.mini { (1.0, false) } else { self.visibility() };
        if self.exit || !(self.dirty || moving || self.ui.pending() || e != self.shown) {
            return;
        }
        self.dirty = false;
        self.shown = e;
        let cv: Canvas = if self.mini { self.ui.render_mini() } else { self.ui.render(self.size.0 as usize, self.size.1 as usize, e) };
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

impl CompositorHandler for Wonder {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Wonder {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Wonder {
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

impl SeatHandler for Wonder {
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

impl KeyboardHandler for Wonder {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key(&event);
        self.draw(qh);
    }
    fn repeat_key(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if !self.ctrl && !matches!(event.keysym, Keysym::Return | Keysym::KP_Enter | Keysym::Escape | Keysym::Tab | Keysym::space) {
            self.key(&event);
            self.draw(qh);
        }
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        self.ctrl = m.ctrl;
    }
}

impl PointerHandler for Wonder {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let at = (e.position.0 as f32, e.position.1 as f32);
            match e.kind {
                PointerEventKind::Press { button: BTN_RIGHT, .. } if self.mini => self.close(), // hides the pill
                PointerEventKind::Press { button: BTN_LEFT, .. } if self.mini => {
                    if let Mini::Expand = self.ui.press_mini(at.0, at.1) {
                        self.next = Next::Full;
                        self.close();
                    }
                    self.dirty = true;
                }
                _ if self.mini => {}
                PointerEventKind::Press { button: BTN_LEFT, .. } => self.dirty |= self.ui.press(at.0, at.1, w, h),
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => self.dirty |= self.ui.hover(Some(at), w, h),
                PointerEventKind::Leave { .. } => self.dirty |= self.ui.hover(None, w, h),
                PointerEventKind::Axis { vertical, .. } if vertical.absolute != 0.0 => {
                    self.ui.move_sel(if vertical.absolute > 0.0 { 1 } else { -1 });
                    self.dirty = true;
                }
                _ => {}
            }
        }
        self.draw(qh);
    }
}

impl ShmHandler for Wonder {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// The player moves by itself, so a thread asks the compositor for a sync callback four times a second; its
/// reply wakes the event loop, which looks at what changed.
struct Tick;

impl Dispatch<wl_callback::WlCallback, Tick> for Wonder {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Tick, _: &Connection, qh: &QueueHandle<Self>) {
        if state.mini && state.raised.try_recv().is_ok() {
            (state.next, state.exit) = (Next::Full, true); // running wonder again opens the whole window
            return;
        }
        if state.ui.tick() {
            state.dirty = true;
            state.draw(qh);
        }
    }
}

delegate_registry!(Wonder);

impl ProvidesRegistryState for Wonder {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Wonder);

fn engine(args: &[String]) -> player::Shared {
    let extra: &[&str] = if args.iter().any(|a| a == "--silent") { &["--ao=null"] } else { &[] };
    player::Player::spawn(extra).expect("mpv did not start: is it installed?")
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--dump-mini") => {
            let cv = Ui::demo(0).render_mini();
            let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
            ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
            std::fs::write(args.get(1).expect("--dump-mini needs a file"), ppm).unwrap();
        }
        Some("--dump") => {
            let size = |i: usize, d: usize| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(d);
            let mut ui = Ui::demo(size(2, 0));
            let cv = ui.render(size(3, 1440), size(4, 900), args.get(5).and_then(|v| v.parse().ok()).unwrap_or(1.0));
            let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
            ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
            std::fs::write(args.get(1).expect("--dump needs a file"), ppm).unwrap();
        }
        Some("search") => match search::search(&args[1..].join(" "), 5) {
            Ok(tracks) => tracks.iter().for_each(|t| println!("{} | {} | {}s | {}", t.title, t.artist, t.secs, t.url())),
            Err(e) => eprintln!("search failed: {e}"),
        },
        // the lyrics of the first search result, to see what the window would show
        Some("lyrics") => {
            let Some(t) = search::search(&args[1..].join(" "), 1).ok().and_then(|r| r.into_iter().next()) else { return eprintln!("no such track") };
            println!("{} ({}s)", t.title, t.secs);
            match lyrics::fetch(&t) {
                Some(l) => println!("{} lines, synced: {}\n{:?}", l.lines.len(), l.synced, &l.lines[..l.lines.len().min(4)]),
                None => println!("no lyrics"),
            }
        }
        // the player alone on the bus; `playerctl -p wonder open <url>` loads a track
        Some("serve") => {
            let player = engine(&args);
            let query = args[1..].iter().filter(|a| *a != "--silent").cloned().collect::<Vec<_>>().join(" ");
            if !query.is_empty() {
                for t in search::search(&query, 3).unwrap_or_default() {
                    player.lock().unwrap().add(t); // the first results of any words after the flags
                }
            }
            let _bus = mpris::serve(player, std::sync::mpsc::channel().0).expect("the MPRIS name is taken or there is no session bus");
            loop {
                std::thread::park();
            }
        }
        _ => window(&args),
    }
}

/// The window comes and goes; the player and its MPRIS name live on, so closing the window keeps the music going.
/// Running `seiza-wonder` again (or MPRIS Raise) brings the window back.
fn window(args: &[String]) {
    if mpris::raise_running() {
        return;
    }
    let player = engine(args);
    let (raise, raised) = std::sync::mpsc::channel();
    let _bus = mpris::serve(player.clone(), raise).map_err(|e| eprintln!("seiza-wonder: no MPRIS ({e})")).ok();
    let raised = Rc::new(raised);
    let mut next = Next::Full;
    loop {
        next = match show(&player, next == Next::Mini, &raised) {
            Next::Gone => break,
            Next::Hide => {
                while raised.try_recv().is_ok() {} // a raise sent while the window was closing
                if raised.recv().is_err() {
                    break;
                }
                Next::Full
            }
            n => n,
        };
    }
}

/// One window (or the mini pill), until it ends; what it asks for next. Gone ends wonder too.
fn show(player: &player::Shared, mini: bool, raised: &Rc<Receiver<()>>) -> Next {
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-wonder"), None);
    if mini {
        layer.set_anchor(Anchor::BOTTOM); // centred, clicks only where the pill is, no keyboard
        layer.set_margin(0, 0, 60, 0);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(ui::MINI.0, ui::MINI.1);
    } else {
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.set_size(0, 0);
    }
    layer.commit();

    while raised.try_recv().is_ok() {} // a raise from before this window is not for it
    let (tick_conn, tick_qh) = (conn.clone(), qh.clone());
    let open = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let still_open = open.clone();
    std::thread::spawn(move || while still_open.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(250));
        tick_conn.display().sync(&tick_qh, Tick);
        let _ = tick_conn.flush();
    });

    let mut app = Wonder {
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
        ctrl: false,
        mini,
        next: Next::Hide,
        raised: raised.clone(),
        anim: Default::default(),
        shown: -1.0,
        ui: {
            let mut ui = Ui::new(Some(player.clone()));
            ui.smooth = mini;
            ui
        },
    };
    while !app.exit {
        if event_queue.blocking_dispatch(&mut app).is_err() {
            app.next = Next::Gone;
            break;
        }
    }
    open.store(false, std::sync::atomic::Ordering::Relaxed);
    app.next
}
