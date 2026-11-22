//! seiza-osd: a small pill near the bottom of the screen that shows the volume or the screen
//! brightness for a moment after either changes. `seiza-osd --dump out.ppm [volume|brightness]`
//! draws one frame to a file.
use std::{
    sync::mpsc::{channel, Receiver},
    time::{Duration, Instant},
};

use seiza_common::{
    draw::{shared_font, Canvas, Text},
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

const W: u32 = 300;
const PILL_H: u32 = 56;
const SLIDE: u32 = 16; // the pill rises this far while it fades in
const H: u32 = PILL_H + SLIDE;
const SHOW_FOR: Duration = Duration::from_millis(1500);

/// What the pill shows: a label, a level 0..=1 and whether it is muted.
#[derive(Clone, Copy, PartialEq)]
struct Level(&'static str, f32, bool);

struct Msg(Level);

/// Move `v` toward `to` like a spring that never overshoots, at the same speed at any frame rate.
fn approach(v: f32, to: f32, k: f32, dt: f32) -> f32 {
    let n = v + (to - v) * (1.0 - (-k * dt).exp());
    if (n - to).abs() < 0.002 { to } else { n }
}

/// (level, muted) of the default output, via PipeWire's wpctl: "Volume: 0.42 [MUTED]".
fn parse_volume(s: &str) -> Option<(f32, bool)> {
    Some((s.split_whitespace().nth(1)?.parse().ok()?, s.contains("[MUTED]")))
}

fn volume() -> Option<Level> {
    let out = std::process::Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    let (v, muted) = parse_volume(&String::from_utf8_lossy(&out.stdout))?;
    Some(Level("volume", v, muted))
}

fn brightness() -> Option<Level> {
    let dir = std::fs::read_dir("/sys/class/backlight").ok()?.next()?.ok()?.path();
    let num = |f: &str| std::fs::read_to_string(dir.join(f)).ok()?.trim().parse::<f32>().ok();
    Some(Level("brightness", num("brightness")? / num("max_brightness")?.max(1.0), false))
}

/// One frame: `frac` is the level shown now, `vis` how far the pill has faded and risen in (0..=1),
/// `t` seconds the pill has been up (it turns the sun).
fn render(l: Level, frac: f32, vis: f32, t: f32, text: &mut Text) -> Canvas {
    let (w, h) = (W as usize, H as usize);
    let mut cv = Canvas { w, h, px: vec![0; w * h] };
    if vis <= 0.0 {
        return cv;
    }
    let p = palette();
    let y0 = SLIDE as f32 * (1.0 - vis);
    let r = PILL_H as f32 / 2.0;
    cv.round_rect(0.0, y0, W as f32, PILL_H as f32, r, rgba(p.text, 0.11));
    cv.round_rect(1.0, y0 + 1.0, W as f32 - 2.0, PILL_H as f32 - 2.0, r - 1.0, rgba(p.panel, 0.92));
    let cy = y0 + r;
    let (ink, gold) = if l.2 { (rgba(p.text, 0.5), rgba(p.text, 0.4)) } else { (rgba(p.text, 1.0), rgba(p.star, 1.0)) }; // muted: grey, not gold
    if l.0 == "brightness" {
        cv.icon_sun(40.0, cy, 14.0, frac * 4.0 * std::f32::consts::PI + t * 0.5, gold);
    } else {
        cv.icon_speaker(40.0, cy, 13.0, frac * 3.0, l.2, gold);
    }
    let (tx, tw) = (72.0, 140.0);
    cv.round_rect(tx, cy - 2.0, tw, 4.0, 2.0, rgba(p.text, 0.12));
    let fill = tw * frac.clamp(0.0, 1.0);
    if fill > 0.0 {
        cv.round_rect(tx, cy - 2.0, fill.max(4.0), 4.0, 2.0, gold);
    }
    // the thumb is a little star in a soft glow
    let thumb = tx + fill.max(2.0);
    cv.round_rect(thumb - 14.0, cy - 14.0, 28.0, 28.0, 14.0, rgba(p.star, if l.2 { 0.04 } else { 0.10 }));
    cv.sparkle(thumb, cy, 9.5, if l.2 { rgba(p.text, 0.45) } else { rgba(p.text, 1.0) });
    let label = if l.2 { "muted".to_string() } else { format!("{}", (frac * 100.0).round()) };
    let lw = text.width(&label);
    text.draw(&mut cv, W as f32 - 32.0 - lw, cy + 5.0, &label, ink);
    let k = (vis.clamp(0.0, 1.0) * 256.0) as u32;
    for px in cv.px.iter_mut() {
        let ch = |shift: u32| (*px >> shift & 0xFF) * k >> 8;
        *px = ch(24) << 24 | ch(16) << 16 | ch(8) << 8 | ch(0);
    }
    cv
}

struct Osd {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    configured: bool,
    frame_pending: bool,
    level: Level,       // what the last change was
    shown_at: Instant,  // when it happened
    vis: f32,           // 0 = gone, 1 = fully up
    frac: f32,          // the level drawn now, gliding toward level.1
    up_for: f32,        // seconds the pill has been up
    last_tick: Instant,
    rx: Receiver<Msg>,
    text: Text, // kept between frames: parsing the font is the slow part of a frame
}

impl Osd {
    fn active(&self) -> bool {
        self.vis > 0.0 || self.shown_at.elapsed() < SHOW_FOR
    }

    /// Advance the animation by the time since the last frame, draw it and ask for the next frame.
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending {
            return;
        }
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.05);
        self.last_tick = now;
        let up = self.shown_at.elapsed() < SHOW_FOR;
        self.vis = approach(self.vis, if up { 1.0 } else { 0.0 }, if up { 18.0 } else { 10.0 }, dt);
        self.frac = approach(self.frac, self.level.1, 20.0, dt);
        if self.vis > 0.0 {
            self.up_for += dt;
        } else {
            self.up_for = 0.0;
        }
        let cv = render(self.level, self.frac, self.vis, self.up_for, &mut self.text);
        let (buffer, canvas) = self.pool.create_buffer(W as i32, H as i32, W as i32 * 4, wl_shm::Format::Argb8888).expect("create buffer");
        let bytes = unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.px.len() * 4) };
        canvas[..bytes.len()].copy_from_slice(bytes);
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, W as i32, H as i32);
        if self.active() {
            surface.frame(qh, FrameCallbackData(surface.clone()));
            self.frame_pending = true;
        }
        buffer.attach_to(surface).expect("buffer attach");
        self.layer.commit();
    }

    fn wake(&mut self, qh: &QueueHandle<Self>) {
        let Some(Msg(l)) = self.rx.try_iter().last() else { return };
        if self.vis == 0.0 || l.0 != self.level.0 {
            self.frac = l.1; // a different thing, or a fresh pill: start at the right level
        }
        if !self.active() {
            self.last_tick = Instant::now();
        }
        (self.level, self.shown_at) = (l, Instant::now());
        self.draw(qh);
    }
}

impl CompositorHandler for Osd {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Osd {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Osd {
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

impl ShmHandler for Osd {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

struct Wake;

impl Dispatch<wl_callback::WlCallback, Wake> for Osd {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Wake, _: &Connection, qh: &QueueHandle<Self>) {
        state.wake(qh);
    }
}

delegate_registry!(Osd);

impl ProvidesRegistryState for Osd {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(Osd);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        // seiza-osd --dump out.ppm [volume|brightness] [percent] [muted]
        let kind = if args.get(2).map(String::as_str) == Some("brightness") { "brightness" } else { "volume" };
        let frac = args.get(3).and_then(|v| v.parse::<f32>().ok()).unwrap_or(42.0) / 100.0;
        let cv = render(Level(kind, frac, args.get(4).is_some()), frac, 1.0, 0.0, &mut Text::new(shared_font(), 15.0));
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
    surface.set_input_region(Some(empty.wl_region())); // clicks go to whatever is behind the pill
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("seiza-osd"), None);
    layer.set_anchor(Anchor::BOTTOM);
    layer.set_margin(0, 0, 80, 0);
    layer.set_exclusive_zone(0);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(W, H);
    layer.commit();

    // shortcut: polls every 250 ms (a wpctl spawn each); switch to `pactl subscribe` if that shows up in a profile.
    let (tx, rx) = channel();
    let (poll_conn, poll_qh) = (conn.clone(), qh.clone());
    std::thread::spawn(move || {
        let read = || (volume(), brightness());
        let mut last = read();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = read();
            let changed = [(now.0, last.0), (now.1, last.1)].into_iter().find_map(|(n, o)| n.filter(|n| Some(*n) != o));
            last = now;
            if let Some(l) = changed {
                if tx.send(Msg(l)).is_ok() {
                    poll_conn.display().sync(&poll_qh, Wake);
                    let _ = poll_conn.flush();
                }
            }
        }
    });

    let mut app = Osd {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        configured: false,
        frame_pending: false,
        level: Level("volume", 0.0, false),
        shown_at: Instant::now() - SHOW_FOR,
        vis: 0.0,
        frac: 0.0,
        up_for: 0.0,
        last_tick: Instant::now(),
        rx,
        text: Text::new(shared_font(), 15.0),
    };
    while event_queue.blocking_dispatch(&mut app).is_ok() {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn wpctl_output_parses() {
        assert_eq!(super::parse_volume("Volume: 0.42\n"), Some((0.42, false)));
        assert_eq!(super::parse_volume("Volume: 0.30 [MUTED]\n"), Some((0.30, true)));
        assert_eq!(super::parse_volume(""), None);
    }
}
