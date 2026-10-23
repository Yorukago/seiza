//! seiza-session: the power menu. A dimmed full-screen overlay with one button
//! per action; the ones that end or interrupt the session ask for a second
//! click (or the same key again) before they run.
//!
//! SEIZA_SESSION_DRY=1 prints the command instead of running it.
use std::process::Command;

use seiza_common::{
    palette::{palette, rgba},
    draw::{load_font, Canvas, Rgba, Text},
    STAR_COLORS,
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

const BTN: f32 = 150.0;
const GAP: f32 = 28.0;
const BTN_LEFT: u32 = 0x110;

struct Action {
    label: &'static str,
    key: &'static str, // the letter that selects it
    confirm: bool,
    run: Run,
}

enum Run {
    Lock,
    Seiza(&'static str),
    Shell(&'static [&'static str]),
}

const ACTIONS: [Action; 5] = [
    Action { label: "Lock", key: "l", confirm: false, run: Run::Lock },
    Action { label: "Log out", key: "o", confirm: true, run: Run::Seiza("seiza quit") },
    Action { label: "Restart", key: "r", confirm: true, run: Run::Shell(&["systemctl", "reboot"]) },
    Action { label: "Shut down", key: "s", confirm: true, run: Run::Shell(&["systemctl", "poweroff"]) },
    Action { label: "UEFI", key: "u", confirm: true, run: Run::Shell(&["systemctl", "reboot", "--firmware-setup"]) },
];

/// Arm-then-confirm: dangerous actions need a second press of the same button.
#[derive(Default)]
struct Menu {
    armed: Option<usize>,
}

#[derive(Debug, PartialEq)]
enum Step {
    Armed,
    Run(usize),
}

impl Menu {
    fn press(&mut self, i: usize) -> Step {
        if !ACTIONS[i].confirm || self.armed == Some(i) {
            return Step::Run(i);
        }
        self.armed = Some(i);
        Step::Armed
    }
}

/// Run an action. Err carries a message to show.
fn run(a: &Action) -> Result<(), String> {
    let dry = std::env::var_os("SEIZA_SESSION_DRY").is_some();
    match &a.run {
        Run::Lock => {
            for locker in ["seiza-lock", "hyprlock", "swaylock", "gtklock", "waylock"] {
                if dry {
                    println!("would run: {locker}");
                    return Ok(());
                }
                if Command::new(locker).spawn().is_ok() {
                    return Ok(());
                }
            }
            Err("no screen locker installed (seiza-lock, hyprlock, swaylock, gtklock or waylock)".into())
        }
        Run::Seiza(cmd) => {
            if dry {
                println!("would send: {cmd}");
                return Ok(());
            }
            seiza_common::command(cmd).map(|_| ()).map_err(|e| e.to_string())
        }
        Run::Shell(args) => {
            if dry {
                println!("would run: {}", args.join(" "));
                return Ok(());
            }
            Command::new(args[0]).args(&args[1..]).spawn().map(|_| ()).map_err(|e| e.to_string())
        }
    }
}

struct Session {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    width: u32,
    height: u32,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    exit: bool,
    menu: Menu,
    hover: Option<usize>,
    error: Option<String>,
    text: Text,
}

impl Session {
    fn button_x(&self, i: usize) -> f32 {
        let total = ACTIONS.len() as f32 * BTN + (ACTIONS.len() - 1) as f32 * GAP;
        (self.width as f32 - total) / 2.0 + i as f32 * (BTN + GAP)
    }

    fn button_y(&self) -> f32 {
        (self.height as f32 - BTN) / 2.0
    }

    fn hit(&self, x: f32, y: f32) -> Option<usize> {
        (0..ACTIONS.len()).find(|&i| x >= self.button_x(i) && x < self.button_x(i) + BTN && y >= self.button_y() && y < self.button_y() + BTN)
    }

    fn render(&mut self) -> Canvas {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut cv = Canvas { w, h, px: vec![0; w * h] };
        cv.round_rect(0.0, 0.0, w as f32, h as f32, 0.0, rgba(palette().sky, 0.82));
        let ink: Rgba = rgba(palette().text, 1.0);
        let dim: Rgba = rgba(palette().text, 0.6);
        let y = self.button_y();
        for (i, a) in ACTIONS.iter().enumerate() {
            let x = self.button_x(i);
            let col = STAR_COLORS[i % 6];
            let armed = self.menu.armed == Some(i);
            let lit = armed || self.hover == Some(i);
            if armed {
                cv.round_rect(x - 3.0, y - 3.0, BTN + 6.0, BTN + 6.0, 31.0, [col[0], col[1], col[2], 0.95]);
                cv.round_rect(x, y, BTN, BTN, 28.0, rgba(palette().panel, 1.0));
            }
            cv.round_rect(x, y, BTN, BTN, 28.0, [col[0], col[1], col[2], if lit { 0.42 } else { 0.16 }]);
            let (cx, cy) = (x + BTN / 2.0, y + BTN / 2.0 - 16.0);
            let icon: Rgba = [col[0], col[1], col[2], 1.0];
            match i {
                0 => cv.icon_lock(cx, cy, 24.0, icon),
                1 => cv.icon_logout(cx, cy, 24.0, icon),
                2 => cv.icon_restart(cx, cy, 24.0, icon),
                3 => cv.icon_power(cx, cy, 24.0, icon),
                _ => cv.icon_chip(cx, cy, 24.0, icon),
            }
            let label = if armed { "Confirm?" } else { a.label };
            let tw = self.text.width(label);
            self.text.draw(&mut cv, x + (BTN - tw) / 2.0, y + BTN - 34.0, label, if armed { icon } else { ink });
            let hint = a.key.to_uppercase();
            let hw = self.text.width(&hint);
            self.text.draw(&mut cv, x + (BTN - hw) / 2.0, y + BTN - 12.0, &hint, dim);
        }
        let msg = self.error.clone().unwrap_or_else(|| "click or press the letter  ·  dangerous ones ask twice  ·  Esc closes".to_string());
        let mw = self.text.width(&msg);
        let color = if self.error.is_some() { [1.0, 0.5, 0.5, 1.0] } else { dim };
        self.text.draw(&mut cv, (w as f32 - mw) / 2.0, y + BTN + 56.0, &msg, color);
        cv
    }

    fn activate(&mut self, i: usize) {
        match self.menu.press(i) {
            Step::Armed => self.dirty = true,
            Step::Run(i) => match run(&ACTIONS[i]) {
                Ok(()) => self.exit = true,
                Err(e) => {
                    self.error = Some(e);
                    self.menu.armed = None;
                    self.dirty = true;
                }
            },
        }
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending || !self.dirty {
            return;
        }
        self.dirty = false;
        let cv = self.render();
        if let Ok(path) = std::env::var("SEIZA_SESSION_DUMP") { // debugging aid: write the menu as PPM and quit
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

impl CompositorHandler for Session {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh); // anything that changed while the last frame was in flight
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Session {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Session {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        let size = (c.new_size.0.max(1), c.new_size.1.max(1));
        // Seiza re-sends the same configure after every commit; redrawing for it would loop forever.
        if self.configured && size == (self.width, self.height) {
            return;
        }
        (self.width, self.height) = size;
        self.configured = true;
        self.dirty = true;
        self.draw(qh);
    }
}

impl SeatHandler for Session {
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

impl KeyboardHandler for Session {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if event.keysym == Keysym::Escape {
            self.exit = true;
        } else if event.keysym == Keysym::Return {
            if let Some(i) = self.menu.armed {
                self.activate(i);
            }
        } else if let Some(i) = event.utf8.as_deref().and_then(|k| ACTIONS.iter().position(|a| a.key == k.to_lowercase())) {
            self.activate(i);
        }
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: RawModifiers, _: u32) {}
}

impl PointerHandler for Session {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let hit = self.hit(e.position.0 as f32, e.position.1 as f32);
            match e.kind {
                PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } if hit != self.hover => {
                    self.hover = hit;
                    self.dirty = true;
                    self.draw(qh);
                }
                PointerEventKind::Press { button: BTN_LEFT, .. } => match hit {
                    Some(i) => self.activate(i),
                    None => self.exit = true, // a click outside closes the menu
                },
                _ => {}
            }
        }
    }
}

impl ShmHandler for Session {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Session);

impl ProvidesRegistryState for Session {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Session);

fn main() {
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-session"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0);
    layer.commit();

    let mut app = Session {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        keyboard: None,
        pointer: None,
        width: 1,
        height: 1,
        configured: false,
        frame_pending: false,
        dirty: true,
        exit: false,
        menu: Menu::default(),
        hover: None,
        error: None,
        text: Text::new(load_font(), 18.0),
    };

    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_runs_at_once_and_the_rest_ask_twice() {
        let mut m = Menu::default();
        assert_eq!(m.press(0), Step::Run(0));
        assert_eq!(m.press(3), Step::Armed);
        assert_eq!(m.press(3), Step::Run(3));
    }

    #[test]
    fn arming_one_button_disarms_the_other() {
        let mut m = Menu::default();
        assert_eq!(m.press(2), Step::Armed);
        assert_eq!(m.press(3), Step::Armed);
        assert_eq!(m.press(2), Step::Armed, "restart must be armed again, not run on the strength of an earlier click");
    }

    #[test]
    fn every_key_is_unique() {
        let mut keys: Vec<_> = ACTIONS.iter().map(|a| a.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), ACTIONS.len());
    }
}
