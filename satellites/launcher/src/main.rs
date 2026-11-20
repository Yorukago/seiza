//! seiza-launcher: type to find an app, a command or a web search, Enter to start it.
//! Two looks, `style=list` or `style=stars` in ~/.config/seiza/launcher (Tab flips
//! it for this opening; `seiza-launcher list|stars` forces one).
//!
//! Starting goes through Seiza's socket, so the app gets Seiza's environment and
//! opens in the current star. SEIZA_LAUNCHER_DRY=1 prints the command instead.
//! `seiza-launcher --dump out.ppm [list|stars] [query]` draws one frame to a file.
mod calc;
mod emoji;
mod ui;

use seiza_common::{apps::load_apps, draw::Canvas};
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
use std::time::Instant;
use ui::{Style, Ui};

const OPEN_SECS: f32 = 0.35;
const CLOSE_SECS: f32 = 0.2;

const BTN_LEFT: u32 = 0x110;

fn config_style() -> Style {
    if seiza_common::config::get("launcher", "style").as_deref() == Some("stars") { Style::Stars } else { Style::List }
}

fn style_arg(s: &str) -> Option<Style> {
    match s {
        "list" => Some(Style::List),
        "stars" => Some(Style::Stars),
        _ => None,
    }
}

/// One launcher at a time: a second run closes the first (so the key toggles it).
/// Returns false when it closed a running one and this process should just exit.
fn claim_singleton() -> bool {
    let path = format!("{}/seiza-launcher.pid", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()));
    if let Some(pid) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
        let ours = std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "seiza-launcher");
        if ours && pid != std::process::id() {
            let _ = std::process::Command::new("kill").arg(pid.to_string()).status();
            return false;
        }
    }
    let _ = std::fs::write(&path, std::process::id().to_string());
    true
}

struct Launcher {
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
    windows: bool, // the window switcher: Tab steps on, letting go of Super picks
    opened: Option<Instant>,
    closing: Option<Instant>,
    shown: f32, // the visibility last drawn
    ui: Ui,
}

impl Launcher {
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

    fn launch(&mut self, i: usize) {
        let Some(line) = self.ui.item(i).map(|it| it.line.clone()) else { return };
        if std::env::var_os("SEIZA_LAUNCHER_DRY").is_some() {
            println!("would send: {line}");
            self.close();
        } else {
            match seiza_common::command(&line) {
                Ok(_) => self.close(),
                Err(e) => self.ui.error = Some(format!("couldn't reach seiza: {e}")),
            }
        }
        self.dirty = true;
    }

    fn key(&mut self, event: &KeyEvent) {
        self.ui.error = None;
        let stars = self.ui.style == Style::Stars;
        match event.keysym {
            Keysym::Escape => self.close(),
            Keysym::Return | Keysym::KP_Enter => {
                let i = self.ui.sel;
                self.launch(i);
            }
            Keysym::Tab if self.windows => self.ui.step(1),
            Keysym::ISO_Left_Tab if self.windows => self.ui.step(-1),
            Keysym::Tab => self.ui.toggle_style(),
            Keysym::BackSpace => {
                self.ui.query.pop();
                self.ui.refresh();
            }
            Keysym::Up if stars => self.ui.drift(0.0, -1.0),
            Keysym::Down if stars => self.ui.drift(0.0, 1.0),
            Keysym::Left if stars => self.ui.drift(-1.0, 0.0),
            Keysym::Right if stars => self.ui.drift(1.0, 0.0),
            Keysym::Up => self.ui.step(-1),
            Keysym::Down => self.ui.step(1),
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
        let moving = moving || self.ui.gliding();
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

impl CompositorHandler for Launcher {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Launcher {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Launcher {
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

impl SeatHandler for Launcher {
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

impl KeyboardHandler for Launcher {
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
        if self.windows && self.chord && !m.alt && !m.ctrl && !m.logo && self.closing.is_none() {
            let i = self.ui.sel;
            self.launch(i);
        }
        self.chord = m.ctrl || m.alt || m.logo;
    }
}

impl PointerHandler for Launcher {
    fn pointer_frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            if &e.surface != self.layer.wl_surface() {
                continue;
            }
            let hit = self.ui.hit(e.position.0 as f32, e.position.1 as f32);
            match e.kind {
                PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                    if let Some(i) = hit.filter(|&i| i != self.ui.sel) {
                        self.ui.select(i);
                        self.dirty = true;
                    }
                }
                PointerEventKind::Press { button: BTN_LEFT, .. } => match hit {
                    Some(i) => self.launch(i),
                    None => self.close(), // a click outside closes it
                },
                PointerEventKind::Axis { vertical, .. } if self.ui.style == Style::List => {
                    self.ui.scroll_by(if vertical.absolute > 0.0 { 1 } else { -1 });
                    self.dirty = true;
                }
                _ => {}
            }
        }
        self.draw(qh);
    }
}

impl ShmHandler for Launcher {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// The star twinkle has no events to ride on, so a thread asks the compositor for
/// a sync callback every 125 ms; its reply wakes the event loop to redraw.
struct Tick;

impl Dispatch<wl_callback::WlCallback, Tick> for Launcher {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Tick, _: &Connection, qh: &QueueHandle<Self>) {
        if state.ui.style == Style::Stars && state.configured {
            state.dirty = true;
            state.draw(qh);
        }
    }
}

delegate_registry!(Launcher);

impl ProvidesRegistryState for Launcher {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Launcher);

/// The clipboard history that scripts/seiza-clip-store keeps in $XDG_RUNTIME_DIR/seiza-clip, newest first, as
/// "apps" whose command puts the entry back on the clipboard.
fn load_clip(dir: &std::path::Path) -> Vec<seiza_common::apps::App> {
    let mut files: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    files.sort_by(|a, b| b.cmp(a)); // names are millisecond stamps
    files
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let first = text.lines().find(|l| !l.trim().is_empty())?.trim();
            let name: String = first.chars().take(90).collect();
            let more = if text.trim().lines().count() > 1 || first.chars().count() > 90 { "…" } else { "" };
            Some(seiza_common::apps::App { name: format!("{name}{more}"), exec: format!("wl-copy < '{}'", p.display()), extra: String::new(), group: seiza_common::apps::Group::System })
        })
        .collect()
}

/// The window list `seiza switch` writes, most recent first: `star<TAB>app<TAB>title` a line.
fn load_windows(file: &std::path::Path) -> Vec<seiza_common::apps::App> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .enumerate()
        .map(|(i, l)| {
            let mut f = l.splitn(3, '\t');
            let (star, app, title) = (f.next().unwrap_or("?"), f.next().unwrap_or(""), f.next().unwrap_or(""));
            let name = if title.is_empty() { app } else { title };
            seiza_common::apps::App { name: format!("{name}  ·  star {star}"), exec: format!("seiza focus-window {}", i + 1), extra: app.to_lowercase(), group: seiza_common::apps::Group::System }
        })
        .collect()
}

fn windows_file() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("seiza-windows")
}

fn clip_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())).join("seiza-clip")
}

fn dump(path: &str, args: &[String]) {
    let style = args.first().and_then(|a| style_arg(a)).unwrap_or(Style::List);
    let mut ui = Ui::new(load_apps(), style, 1920.0, 1080.0);
    ui.query = args.get(1).cloned().unwrap_or_default();
    ui.refresh();
    let cv: Canvas = ui.render(args.get(2).and_then(|e| e.parse().ok()).unwrap_or(1.0));
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
    let clip = args.iter().any(|a| a == "--clip");
    let windows = args.iter().any(|a| a == "--windows");
    let style = args.first().and_then(|a| style_arg(a)).unwrap_or(if clip || windows { Style::List } else { config_style() });

    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let layer = layer_shell.create_layer_surface(&qh, compositor.create_surface(&qh), Layer::Overlay, Some("seiza-launcher"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0);
    layer.commit();

    let (tick_conn, tick_qh) = (conn.clone(), qh.clone());
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(125));
        tick_conn.display().sync(&tick_qh, Tick);
        let _ = tick_conn.flush();
    });

    let mut app = Launcher {
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
        windows,
        opened: None,
        closing: None,
        shown: 0.0,
        ui: Ui::new(if clip { load_clip(&clip_dir()) } else if windows { load_windows(&windows_file()) } else { load_apps() }, style, 1.0, 1.0),
    };
    app.ui.picker = clip || windows;
    app.ui.refresh();
    if windows && app.ui.item(1).is_some() {
        app.ui.select(1); // the window before this one: Super+Tab flips back and forth
    }

    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
    let _ = std::fs::remove_file(format!("{}/seiza-launcher.pid", std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())));
}

#[cfg(test)]
mod tests {
    #[test]
    fn windows_list_keeps_recency_numbers() {
        let f = std::env::temp_dir().join(format!("seiza-win-test-{}", std::process::id()));
        std::fs::write(&f, "1\tkitty\tfish\n2\tFirefox\t\n").unwrap();
        let apps = super::load_windows(&f);
        assert_eq!((apps[0].name.as_str(), apps[0].exec.as_str()), ("fish  ·  star 1", "seiza focus-window 1"));
        assert_eq!((apps[1].name.as_str(), apps[1].extra.as_str()), ("Firefox  ·  star 2", "firefox"));
        std::fs::remove_file(f).unwrap();
    }

    #[test]
    fn clip_history_is_newest_first_and_puts_the_text_back() {
        let d = std::env::temp_dir().join(format!("seiza-clip-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("1000.txt"), "old").unwrap();
        std::fs::write(d.join("2000.txt"), "\nnew line\nsecond").unwrap();
        let apps = super::load_clip(&d);
        assert_eq!(apps[0].name, "new line…");
        assert_eq!(apps[1].name, "old");
        assert_eq!(apps[1].exec, format!("wl-copy < '{}'", d.join("1000.txt").display()));
        std::fs::remove_dir_all(d).unwrap();
    }
}
