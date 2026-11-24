//! A track's cover: its YouTube thumbnail, kept in the cache and cut to the card's rounded frame.
use crate::search::Track;
use seiza_common::draw::Canvas;
use std::process::Command;

pub const W: usize = 360;
pub const H: usize = 203;
const R: f32 = 14.0;

/// Blocks on the network the first time: call it from a thread.
pub fn fetch(t: &Track) -> Option<Canvas> {
    if t.id.is_empty() || !t.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None; // the id becomes a file name
    }
    let cache = crate::cache::root()?;
    std::fs::create_dir_all(&cache).ok()?;
    let file = cache.join(format!("{}.jpg", t.id));
    if !file.exists() {
        let ok = Command::new("curl").args(["-s", "-m", "15", "-f", "-A", "seiza-wonder", "-o"]).arg(&file).arg(t.thumbnail()).status().ok()?.success();
        if !ok {
            let _ = std::fs::remove_file(&file);
            return None;
        }
    }
    let sz = format!("{W}x{H}");
    let out = Command::new("magick").arg(format!("{}[0]", file.display())).args(["-resize", &format!("{sz}^"), "-gravity", "center", "-extent", &sz, "ppm:-"]).output().ok()?;
    let nl = out.stdout.iter().enumerate().filter(|(_, b)| **b == b'\n').nth(2)?.0 + 1;
    let rgb = out.stdout.get(nl..nl + W * H * 3)?;
    let px = rgb.chunks(3).enumerate().map(|(i, c)| {
        let (x, y) = ((i % W) as f32 + 0.5, (i / W) as f32 + 0.5);
        let (dx, dy) = ((R - x).max(x - (W as f32 - R)).max(0.0), (R - y).max(y - (H as f32 - R)).max(0.0));
        if dx * dx + dy * dy > R * R { 0 } else { 0xFF00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32 }
    });
    Some(Canvas { w: W, h: H, px: px.collect() })
}
