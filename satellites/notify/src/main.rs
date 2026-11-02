//! seiza-notify: the desktop notification daemon (org.freedesktop.Notifications). Toasts stack under
//! the bar at the top right and fade away; `notify-send` and every app that talks D-Bus lands here.
//! If another daemon (mako, dunst) already owns the name this exits. `seiza-notify --dump out.ppm` draws a sample.
//! shortcut: no actions, icons or click-to-dismiss, and history is only a log file for seiza-panel.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc::{channel, Receiver, Sender},
        Mutex,
    },
    time::{Duration, Instant},
};

use seiza_common::{
    draw::{load_font, Canvas, Text},
    palette::{palette, rgba},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::client::{
        globals::registry_queue_init,
        protocol::{wl_callback, wl_output, wl_shm, wl_surface},
        Connection, Dispatch, QueueHandle,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use zbus::zvariant::Value;

const W: u32 = 340;
const TOAST_H: u32 = 92;
const GAP: u32 = 10;
const MAX_TOASTS: usize = 4;
const H: u32 = MAX_TOASTS as u32 * (TOAST_H + GAP);
const FADE: f32 = 0.25; // seconds to fade in and out
const PATH: &str = "/org/freedesktop/Notifications";
const IFACE: &str = "org.freedesktop.Notifications";

#[derive(Clone)]
struct Toast {
    id: u32,
    app: String,
    summary: String,
    body: String,
    critical: bool,
    born: Instant,
    ttl: Duration,
}

enum Msg {
    Show(Toast),
    Close(u32),
}

/// Words wrapped greedily into at most `lines` lines of `max_w` pixels; the last line ends in "…" if cut.
fn fit(t: &mut Text, s: &str, max_w: f32, lines: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    for word in s.split_whitespace() {
        let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if t.width(&next) <= max_w || cur.is_empty() {
            cur = next;
        } else {
            out.push(std::mem::replace(&mut cur, word.to_string()));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    let cut = out.len() > lines;
    out.truncate(lines);
    for (i, l) in out.iter_mut().enumerate() {
        let last_cut = cut && i + 1 == lines;
        while t.width(l) + if last_cut { 10.0 } else { 0.0 } > max_w && l.chars().count() > 1 {
            l.pop(); // a word wider than the toast, or room for the "…"
            *l = l.trim_end().to_string();
        }
        if last_cut {
            l.push('…');
        }
    }
    out
}

/// Fade the rows y0..y1 of the canvas to `k` (0..=1); the pixels are premultiplied ARGB.
fn fade(cv: &mut Canvas, y0: usize, y1: usize, k: f32) {
    let k = (k.clamp(0.0, 1.0) * 256.0) as u32;
    for px in cv.px[y0 * cv.w..y1.min(cv.h) * cv.w].iter_mut() {
        let ch = |shift: u32| (*px >> shift & 0xFF) * k >> 8;
        *px = ch(24) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0);
    }
}

/// One line per notification in $XDG_RUNTIME_DIR/seiza-notifications (unix time, app, summary, body, tab
/// separated), which seiza-panel lists. It lives in the runtime dir, so a reboot clears it.
fn log(t: &Toast) {
    use std::io::Write;
    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") else { return };
    let one = |s: &str| s.replace(['\t', '\n', '\r'], " ");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::path::Path::new(&dir).join("seiza-notifications")) {
        let _ = writeln!(f, "{now}\t{}\t{}\t{}", one(&t.app), one(&t.summary), one(&t.body));
    }
}

fn render(toasts: &[Toast], now: Instant) -> Canvas {
    let (w, h) = (W as usize, H as usize);
    let mut cv = Canvas { w, h, px: vec![0; w * h] };
    let p = palette();
    let mut small = Text::new(load_font(), 11.0);
    let mut title = Text::new(load_font(), 15.0);
    let mut body = Text::new(load_font(), 13.0);
    for (i, t) in toasts.iter().take(MAX_TOASTS).enumerate() {
        let y = (i as u32 * (TOAST_H + GAP)) as f32;
        let age = now.duration_since(t.born).as_secs_f32();
        let left = t.ttl.as_secs_f32() - age;
        let vis = (age / FADE).min(left / FADE).clamp(0.0, 1.0);
        cv.round_rect(0.0, y, W as f32, TOAST_H as f32, 16.0, rgba(p.panel, 0.94));
        let accent = if t.critical { p.pink } else { p.star };
        cv.star5(24.0, y + 24.0, 8.0, rgba(accent, 1.0));
        let (x, tw) = (44.0, W as f32 - 44.0 - 16.0);
        small.draw(&mut cv, x, y + 27.0, &t.app.to_lowercase(), rgba(p.text, 0.5));
        let head = fit(&mut title, &t.summary, tw, 1);
        title.draw(&mut cv, x, y + 47.0, head.first().map_or("", String::as_str), rgba(p.text, 1.0));
        for (j, l) in fit(&mut body, &t.body, tw, 2).iter().enumerate() {
            body.draw(&mut cv, x, y + 66.0 + 16.0 * j as f32, l, rgba(p.text, 0.75));
        }
        fade(&mut cv, y as usize, (y as usize) + TOAST_H as usize, vis);
    }
    cv
}

struct Notify {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    configured: bool,
    frame_pending: bool,
    toasts: Vec<Toast>,
    rx: Receiver<Msg>,
    bus: Option<zbus::blocking::Connection>,
}

impl Notify {
    fn signal_closed(&self, id: u32, reason: u32) {
        if let Some(bus) = &self.bus {
            let _ = bus.emit_signal(None::<&str>, PATH, IFACE, "NotificationClosed", &(id, reason));
        }
    }

    fn drain(&mut self) {
        let msgs: Vec<Msg> = self.rx.try_iter().collect();
        for m in msgs {
            match m {
                Msg::Show(t) => match self.toasts.iter_mut().find(|o| o.id == t.id) {
                    Some(old) => *old = Toast { born: old.born, ..t }, // a replace keeps its place and its fade
                    None => {
                        log(&t);
                        self.toasts.insert(0, t)
                    }
                },
                Msg::Close(id) => {
                    self.toasts.retain(|t| t.id != id);
                    self.signal_closed(id, 3);
                }
            }
        }
        let now = Instant::now();
        let gone: Vec<u32> = self.toasts.iter().filter(|t| now.duration_since(t.born) >= t.ttl).map(|t| t.id).collect();
        self.toasts.retain(|t| !gone.contains(&t.id));
        for id in gone {
            self.signal_closed(id, 1);
        }
        self.toasts.truncate(MAX_TOASTS);
    }

    /// Redraw, and keep asking for frames while a toast is up (the fades need them).
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending {
            return;
        }
        self.drain();
        let cv = render(&self.toasts, Instant::now());
        let (buffer, canvas) = self.pool.create_buffer(W as i32, H as i32, W as i32 * 4, wl_shm::Format::Argb8888).expect("create buffer");
        let bytes = unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.px.len() * 4) };
        canvas[..bytes.len()].copy_from_slice(bytes);
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, W as i32, H as i32);
        if !self.toasts.is_empty() {
            surface.frame(qh, FrameCallbackData(surface.clone()));
            self.frame_pending = true;
        }
        buffer.attach_to(surface).expect("buffer attach");
        self.layer.commit();
    }
}

impl CompositorHandler for Notify {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Notify {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Notify {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        std::process::exit(0);
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        if !self.configured {
            self.configured = true;
            self.draw(qh);
        }
    }
}

impl ShmHandler for Notify {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

struct Wake;

impl Dispatch<wl_callback::WlCallback, Wake> for Notify {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Wake, _: &Connection, qh: &QueueHandle<Self>) {
        state.frame_pending = false; // a toast arriving while idle: no frame callback is outstanding
        state.draw(qh);
    }
}

delegate_registry!(Notify);

impl ProvidesRegistryState for Notify {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(Notify);

/// The D-Bus side: turns calls into messages for the Wayland thread and pokes it awake.
struct Server {
    next: AtomicU32,
    tx: Mutex<Sender<Msg>>,
    conn: Connection,
    qh: QueueHandle<Notify>,
}

impl Server {
    fn send(&self, m: Msg) {
        if self.tx.lock().unwrap().send(m).is_ok() {
            self.conn.display().sync(&self.qh, Wake);
            let _ = self.conn.flush();
        }
    }
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&str> {
        vec!["body"]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(&self, app: &str, replaces: u32, _icon: &str, summary: &str, body: &str, _actions: Vec<&str>, hints: HashMap<&str, Value<'_>>, timeout: i32) -> u32 {
        let id = if replaces != 0 { replaces } else { self.next.fetch_add(1, Ordering::Relaxed) };
        let critical = hints.get("urgency").and_then(|v| u8::try_from(v).ok()) == Some(2);
        // -1 = the daemon's choice, 0 = never expires (capped: there is no way to dismiss a toast yet)
        let ttl = Duration::from_millis(match timeout { t if t > 0 => t as u64, 0 => 30_000, _ if critical => 15_000, _ => 6_000 });
        self.send(Msg::Show(Toast { id, app: app.into(), summary: summary.into(), body: body.into(), critical, born: Instant::now(), ttl }));
        id
    }

    fn close_notification(&self, id: u32) {
        self.send(Msg::Close(id));
    }

    fn get_server_information(&self) -> (&str, &str, &str, &str) {
        ("seiza", "seiza", env!("CARGO_PKG_VERSION"), "1.2")
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let t = |id, summary: &str, body: &str, critical| Toast { id, app: "notify-send".into(), summary: summary.into(), body: body.into(), critical, born: Instant::now() - Duration::from_secs(1), ttl: Duration::from_secs(9) };
        let toasts = [t(1, "seiza.kdl has 1 mistake(s)", "line 12: unknown key \"zoom\" in the motion block, so the rest of the file was still applied", true), t(2, "Download finished", "debian-13.iso", false)];
        let cv = render(&toasts, Instant::now());
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

    let surface = compositor.create_surface(&qh);
    let empty = Region::new(&compositor).expect("wl_region");
    surface.set_input_region(Some(empty.wl_region())); // clicks go to whatever is behind the toasts
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("seiza-notify"), None);
    layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
    layer.set_margin(56, 16, 0, 0);
    layer.set_exclusive_zone(0);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(W, H);
    layer.commit();

    let (tx, rx) = channel();
    let server = Server { next: AtomicU32::new(1), tx: Mutex::new(tx), conn: conn.clone(), qh: qh.clone() };
    let bus = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(IFACE))
        .and_then(|b| b.serve_at(PATH, server))
        .and_then(|b| b.build());
    let bus = match bus {
        Ok(b) => b,
        Err(e) => return eprintln!("seiza-notify: not the notification daemon ({e})"),
    };

    let mut app = Notify {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        configured: false,
        frame_pending: false,
        toasts: vec![],
        rx,
        bus: Some(bus),
    };
    loop {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_text_wraps_and_cuts() {
        let mut t = Text::new(load_font(), 13.0);
        let lines = fit(&mut t, "one two three four five six seven eight nine ten eleven twelve", 120.0, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'));
        assert!(lines.iter().all(|l| t.width(l) <= 120.0));
        assert_eq!(fit(&mut t, "short", 120.0, 2), ["short"]);
        assert!(fit(&mut t, "", 120.0, 2).is_empty());
    }
}
