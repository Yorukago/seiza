//! Tray icons: this process is the StatusNotifierWatcher and the host, so apps like Steam and Discord
//! can put an icon in the bar. If another watcher already owns the name (waybar, say), there is no tray.
//! shortcut: icons refresh every second rather than on NewIcon, themes other than hicolor are not searched,
//! and a right click calls ContextMenu, so apps that only offer a dbusmenu show no menu; upgrade when it bites.
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use smithay_client_toolkit::reexports::calloop::channel;
use zbus::{blocking::Connection, message::Header};

pub const ICON: usize = 18;
const SNI: &str = "org.kde.StatusNotifierItem";

#[derive(Clone)]
pub struct Item {
    pub dest: String,
    pub path: String,
    pub label: String,
    pub px: Vec<u32>, // ICON x ICON premultiplied ARGB; empty when no icon could be found
    key: (String, String, Vec<u8>), // what the icon was made from, so it is only rebuilt when that changes
}

struct Watcher {
    regs: Arc<Mutex<Vec<(String, String)>>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    /// `service` is a bus name, or a path on the sender's bus name. Only noted here: the properties are read
    /// later, because asking the app now, while it waits for this reply, would deadlock.
    fn register_status_notifier_item(&self, service: &str, #[zbus(header)] hdr: Header<'_>) {
        let sender = hdr.sender().map(|s| s.to_string()).unwrap_or_default();
        let reg = if service.starts_with('/') {
            (sender, service.to_string())
        } else if let Some((d, p)) = service.split_once('/') {
            (d.to_string(), format!("/{p}"))
        } else {
            (service.to_string(), "/StatusNotifierItem".to_string())
        };
        let mut regs = self.regs.lock().unwrap();
        if !regs.contains(&reg) {
            regs.push(reg);
        }
    }

    fn register_status_notifier_host(&self, _service: &str) {}

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.regs.lock().unwrap().iter().map(|(d, p)| format!("{d}{p}")).collect()
    }
}

pub struct Tray {
    pub items: Arc<Mutex<Vec<Item>>>,
    conn: Connection,
}

impl Tray {
    /// Claims the watcher name and starts the refresher; `wake` is poked when the icons change.
    pub fn start(wake: channel::Sender<()>) -> Option<Tray> {
        let regs = Arc::new(Mutex::new(Vec::new()));
        let built = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.name("org.kde.StatusNotifierWatcher"))
            .and_then(|b| b.serve_at("/StatusNotifierWatcher", Watcher { regs: regs.clone() }))
            .and_then(|b| b.build());
        let conn = built.inspect_err(|e| eprintln!("seiza-bar: no tray ({e})")).ok()?;
        let items = Arc::new(Mutex::new(Vec::new()));
        let (c, shared) = (conn.clone(), items.clone());
        thread::spawn(move || refresh_forever(c, regs, shared, wake));
        Some(Tray { items, conn })
    }

    /// `Activate` (left), `ContextMenu` (right) or `SecondaryActivate` (middle) on item `i`.
    pub fn click(&self, i: usize, method: &'static str) {
        let Some(it) = self.items.lock().unwrap().get(i).cloned() else { return };
        let conn = self.conn.clone();
        thread::spawn(move || {
            let _ = conn.call_method(Some(it.dest.as_str()), it.path.as_str(), Some(SNI), method, &(0i32, 0i32));
        });
    }
}

fn refresh_forever(conn: Connection, regs: Arc<Mutex<Vec<(String, String)>>>, shared: Arc<Mutex<Vec<Item>>>, wake: channel::Sender<()>) {
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return };
    loop {
        thread::sleep(Duration::from_secs(1));
        regs.lock().unwrap().retain(|(d, _)| zbus::names::BusName::try_from(d.as_str()).is_ok_and(|n| dbus.name_has_owner(n).unwrap_or(false)));
        let wanted = regs.lock().unwrap().clone();
        let old = shared.lock().unwrap().clone();
        let mut next = Vec::new();
        for (dest, path) in wanted {
            let Ok(p) = zbus::blocking::proxy::Builder::<zbus::blocking::Proxy>::new(&conn)
                .destination(dest.clone())
                .and_then(|b| b.path(path.clone()))
                .and_then(|b| b.interface(SNI))
                .and_then(|b| b.cache_properties(zbus::proxy::CacheProperties::No).build())
            else {
                continue;
            };
            if p.get_property::<String>("Status").is_ok_and(|s| s == "Passive") {
                continue;
            }
            let name = p.get_property::<String>("IconName").unwrap_or_default();
            let theme = p.get_property::<String>("IconThemePath").unwrap_or_default();
            let pixmap = p.get_property::<Vec<(i32, i32, Vec<u8>)>>("IconPixmap").unwrap_or_default();
            let biggest = pixmap.into_iter().max_by_key(|(w, h, _)| w * h);
            let key = (name.clone(), theme.clone(), biggest.as_ref().map(|b| b.2.clone()).unwrap_or_default());
            let label = ["Title", "Id"].iter().filter_map(|k| p.get_property::<String>(k).ok()).find(|s| !s.is_empty()).unwrap_or_default();
            let px = match old.iter().find(|o| o.dest == dest && o.path == path && o.key == key) {
                Some(o) => o.px.clone(),
                None => load_icon(&name, &theme, biggest),
            };
            next.push(Item { dest, path, label, px, key });
        }
        let changed = old.len() != next.len() || old.iter().zip(&next).any(|(a, b)| a.px != b.px || a.label != b.label || a.dest != b.dest);
        if changed {
            *shared.lock().unwrap() = next;
            let _ = wake.send(());
        }
    }
}

/// The app's own pixmap if it sent one, else a png of that name from the icon folders.
fn load_icon(name: &str, theme: &str, pixmap: Option<(i32, i32, Vec<u8>)>) -> Vec<u32> {
    if let Some((w, h, b)) = pixmap.filter(|(w, h, b)| *w > 0 && *h > 0 && b.len() >= (*w * *h * 4) as usize) {
        // ARGB32 in network order: a, r, g, b bytes
        let argb: Vec<[u8; 4]> = b.as_chunks::<4>().0.iter().map(|c| [c[1], c[2], c[3], c[0]]).collect();
        return shrink(&argb, w as usize, h as usize);
    }
    find_png(name, theme).and_then(|p| decode_png(&p)).map(|(w, h, rgba)| shrink(&rgba, w, h)).unwrap_or_default()
}

fn find_png(name: &str, theme: &str) -> Option<std::path::PathBuf> {
    if name.starts_with('/') {
        return Some(name.into());
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs = vec![std::path::PathBuf::from(theme)];
    for root in [format!("{home}/.local/share/icons/hicolor"), "/usr/share/icons/hicolor".to_string()] {
        for size in ["48x48", "32x32", "64x64", "24x24", "22x22", "128x128", "256x256", "16x16"] {
            for kind in ["apps", "status"] {
                dirs.push(format!("{root}/{size}/{kind}").into());
            }
        }
    }
    dirs.push("/usr/share/pixmaps".into());
    dirs.into_iter().filter(|_| !name.is_empty()).map(|d| d.join(format!("{name}.png"))).find(|p| p.is_file())
}

fn decode_png(path: &std::path::Path) -> Option<(usize, usize, Vec<[u8; 4]>)> {
    let mut dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).ok()?));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let n = info.color_type.samples();
    let px = buf[..info.buffer_size()]
        .chunks_exact(n)
        .map(|c| match n {
            1 => [c[0], c[0], c[0], 255],
            2 => [c[0], c[0], c[0], c[1]],
            3 => [c[0], c[1], c[2], 255],
            _ => [c[0], c[1], c[2], c[3]],
        })
        .collect();
    Some((info.width as usize, info.height as usize, px))
}

/// Box-average `src` (r, g, b, a bytes, straight alpha) down to ICON x ICON, as premultiplied ARGB.
fn shrink(src: &[[u8; 4]], w: usize, h: usize) -> Vec<u32> {
    let mut out = Vec::with_capacity(ICON * ICON);
    for y in 0..ICON {
        for x in 0..ICON {
            let (x0, x1) = (x * w / ICON, ((x + 1) * w).div_ceil(ICON).max(x * w / ICON + 1).min(w));
            let (y0, y1) = (y * h / ICON, ((y + 1) * h).div_ceil(ICON).max(y * h / ICON + 1).min(h));
            let mut acc = [0u32; 4]; // premultiplied r, g, b, and a
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = src[sy * w + sx];
                    let a = p[3] as u32;
                    acc[0] += p[0] as u32 * a / 255;
                    acc[1] += p[1] as u32 * a / 255;
                    acc[2] += p[2] as u32 * a / 255;
                    acc[3] += a;
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as u32;
            out.push((acc[3] / n) << 24 | (acc[0] / n) << 16 | (acc[1] / n) << 8 | (acc[2] / n));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_averages_and_premultiplies() {
        // a 36x36 half-transparent red square becomes 18x18 of the same premultiplied pixel
        let px = shrink(&vec![[255, 0, 0, 128]; 36 * 36], 36, 36);
        assert_eq!(px.len(), ICON * ICON);
        assert_eq!(px[0], 128 << 24 | 128 << 16);
    }
}
