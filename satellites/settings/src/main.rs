//! seiza-settings: change how Seiza looks and moves. Every setting is written to the
//! small config file of the program that reads it (see `seiza_common::config`);
//! motion is applied at once, the sky and widgets when this page closes.
//! `seiza-settings --dump out.ppm [page]` draws one frame to a file.
mod ui;

use std::process::Command;

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
use ui::{Effect, Ui};

const BTN_LEFT: u32 = 0x110;

struct Settings {
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
    down: bool,  // left button held: dragging a slider
    ui: Ui,
}

impl Settings {
    fn run(&mut self, effect: Effect) {
        match effect {
            Effect::None => {}
            Effect::ReloadConfig => {
                let _ = seiza_common::command("seiza reload-config");
            }
            Effect::ApplyColors => {
                let path = self.ui.values["look/wallpaper"].trim().replace('~', &std::env::var("HOME").unwrap_or_default());
                self.ui.msg = match Command::new("seiza-wallpaper").arg(&path).output() {
                    Ok(o) if o.status.success() => {
                        for p in ["seiza-starfield", "seiza-bar", "seiza-widgets"] {
                            if !self.ui.restart.contains(&p) {
                                self.ui.restart.push(p);
                            }
                        }
                        "colors borrowed.. they show up when this closes.".into()
                    }
                    Ok(o) => format!("wallust said: {}", String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("no")),
                    Err(e) => format!("couldn't run seiza-wallpaper: {e}"),
                };
            }
        }
        self.dirty = true;
    }

    fn key(&mut self, event: &KeyEvent) {
        let typing = self.ui.focus.is_some_and(|id| id == "look/wallpaper");
        match event.keysym {
            Keysym::Escape if typing => self.ui.focus = None,
            Keysym::Escape => self.exit = true,
            Keysym::Return | Keysym::KP_Enter if typing => {
                self.ui.focus = None;
                let e = self.ui.set("look/apply", "1".into());
                self.run(e);
            }
            Keysym::BackSpace => self.ui.backspace(),
            Keysym::Up if !typing => self.ui.page = self.ui.page.saturating_sub(1),
            Keysym::Down if !typing => self.ui.page = (self.ui.page + 1).min(self.ui.page_names().len() - 1),
            _ => match event.utf8.as_deref() {
                Some(s) if typing && !self.chord && !s.chars().any(char::is_control) => self.ui.type_text(s),
                _ => return,
            },
        }
        self.dirty = true;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending || !self.dirty {
            return;
        }
        self.dirty = false;
        let cv: Canvas = self.ui.render(self.size.0 as usize, self.size.1 as usize);
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

impl CompositorHandler for Settings {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Settings {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Settings {
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

impl SeatHandler for Settings {
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

impl KeyboardHandler for Settings {
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

impl PointerHandler for Settings {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let (x, y) = (e.position.0 as f32, e.position.1 as f32);
            match e.kind {
                PointerEventKind::Press { button: BTN_LEFT, .. } => {
                    self.down = true;
                    let effect = self.ui.press(x, y, w, h, false);
                    self.run(effect);
                }
                PointerEventKind::Motion { .. } if self.down => {
                    let effect = self.ui.press(x, y, w, h, true);
                    self.run(effect);
                }
                PointerEventKind::Release { button: BTN_LEFT, .. } => {
                    self.down = false;
                    self.ui.release();
                    self.dirty = true;
                }
                _ => {}
            }
        }
        self.draw(qh);
    }
}

impl ShmHandler for Settings {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Settings);

impl ProvidesRegistryState for Settings {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Settings);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let mut ui = Ui::new();
        ui.page = args.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
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

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-settings"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0);
    layer.commit();

    let mut app = Settings {
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
        ui: Ui::new(),
    };
    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
    // The sky and widgets read their files at start, so they come back with the new settings.
    for name in &app.ui.restart {
        let _ = Command::new("pkill").args(["-x", name]).status();
        let _ = seiza_common::command(name);
    }
}
