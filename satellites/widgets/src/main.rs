//! seiza-widgets: a clock with the moon's phase, a calendar, the machine's pulse and what is playing,
//! as cards on the sky, under your windows. Which ones, and in what order, is
//! `~/.config/seiza/widgets`: one of clock / calendar / system / media per line
//! (an empty file shows nothing; no file shows all four).
//! `seiza-widgets --dump out.ppm` draws one frame to a file.
mod ui;

use std::time::Duration;

use seiza_common::sys::{read_cpu_counters, read_load, read_ram_pct, read_temp};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
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
use ui::{parse_media, Media, Pulse, Ui, MEDIA_FORMAT, STRIP_H};

fn config_cards() -> Vec<String> {
    let dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| format!("{}/.config", std::env::var("HOME").unwrap_or_default()));
    match std::fs::read_to_string(format!("{dir}/seiza/widgets")) {
        Ok(text) => text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect(),
        Err(_) => ["clock", "calendar", "system", "media"].map(String::from).to_vec(),
    }
}

// shortcut: one playerctl spawn a second; switch to `playerctl --follow` if it ever shows in a profile.
fn read_media() -> Option<Media> {
    let out = std::process::Command::new("playerctl").args(["-a", "metadata", "--format", MEDIA_FORMAT]).output().ok()?;
    parse_media(&String::from_utf8_lossy(&out.stdout))
}

struct Widgets {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    size: (u32, u32),
    configured: bool,
    shown: String,
    cpu_prev: Option<(u64, u64)>,
    ui: Ui,
}

impl Widgets {
    fn draw(&mut self) {
        if !self.configured {
            return;
        }
        let (w, h) = self.size;
        self.shown = self.ui.key();
        let cv = self.ui.render(w as usize, h as usize);
        let (buffer, canvas) = self.pool.create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888).expect("create buffer");
        // shortcut: assumes a little-endian host, which Argb8888 in memory then matches.
        let bytes = unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.px.len() * 4) };
        canvas[..bytes.len()].copy_from_slice(bytes);
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, w as i32, h as i32);
        buffer.attach_to(surface).expect("buffer attach");
        self.layer.commit();
    }

    /// Once a second: read the machine, redraw if anything shows differently.
    fn tick(&mut self) {
        let counters = read_cpu_counters();
        let cpu = match (self.cpu_prev, counters) {
            (Some((t0, i0)), Some((t1, i1))) if t1 > t0 => Some((100 * ((t1 - t0) - (i1 - i0)) / (t1 - t0)) as u32),
            _ => None,
        };
        self.cpu_prev = counters;
        self.ui.set_pulse(Pulse { cpu, ram: read_ram_pct(), temp: read_temp(), load: read_load() });
        if self.ui.cards.iter().any(|c| c == "media") {
            self.ui.media = read_media();
        }
        if self.ui.key() != self.shown {
            self.draw();
        }
    }
}

impl CompositorHandler for Widgets {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Widgets {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Widgets {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        std::process::exit(0);
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, c: LayerSurfaceConfigure, _: u32) {
        let size = (c.new_size.0.max(1), c.new_size.1.max(1));
        // Seiza re-sends the same configure after every commit; redrawing for it would loop forever.
        if self.configured && size == self.size {
            return;
        }
        self.size = size;
        self.configured = true;
        self.draw();
    }
}

impl ShmHandler for Widgets {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// The machine has no events to ride on, so a thread asks the compositor for a sync
/// callback every second; the reply wakes the event loop to read it.
struct Tick;

impl Dispatch<wl_callback::WlCallback, Tick> for Widgets {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Tick, _: &Connection, _: &QueueHandle<Self>) {
        state.tick();
    }
}

delegate_registry!(Widgets);

impl ProvidesRegistryState for Widgets {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(Widgets);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        let mut ui = Ui::new(config_cards());
        ui.set_pulse(Pulse { cpu: read_cpu_counters().map(|_| 23), ram: read_ram_pct(), temp: read_temp(), load: read_load() });
        ui.media = read_media();
        let cv = ui.render(1920, STRIP_H as usize);
        let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
        ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
        std::fs::write(args.get(1).expect("--dump needs a file"), ppm).unwrap();
        return;
    }
    let ui = Ui::new(config_cards());
    if ui.width() <= 1 {
        return; // no widgets asked for: nothing to show
    }
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("compositor has no layer-shell");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let surface = compositor.create_surface(&qh);
    let empty = Region::new(&compositor).expect("wl_region");
    surface.set_input_region(Some(empty.wl_region())); // clicks go to whatever is behind the cards
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Bottom, Some("seiza-widgets"), None);
    layer.set_anchor(Anchor::TOP);
    layer.set_margin(56, 0, 0, 0); // under the bar
    layer.set_exclusive_zone(0);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(ui.width(), STRIP_H);
    layer.commit();

    let (tick_conn, tick_qh) = (conn.clone(), qh.clone());
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        tick_conn.display().sync(&tick_qh, Tick);
        let _ = tick_conn.flush();
    });

    let mut app = Widgets {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        layer,
        size: (1, 1),
        configured: false,
        shown: String::new(),
        cpu_prev: read_cpu_counters(),
        ui,
    };
    loop {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn no_file_means_all_three_cards() {
        let dir = std::env::temp_dir().join(format!("seiza-widgets-test-{}", std::process::id()));
        // SAFETY: tests in this crate do not run in parallel with other readers of this variable.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &dir) };
        assert_eq!(super::config_cards(), ["clock", "calendar", "system", "media"]);
        std::fs::create_dir_all(dir.join("seiza")).unwrap();
        std::fs::write(dir.join("seiza/widgets"), "# mine\ncalendar\n\nclock\n").unwrap();
        assert_eq!(super::config_cards(), ["calendar", "clock"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
