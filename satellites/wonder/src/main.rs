//! wonder: the Seiza music player. For now only its engine (mpv + yt-dlp) and a terminal harness for it;
//! the window comes later.
mod ipc;
mod player;
mod search;

use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("search") => match search::search(&args[1..].join(" "), 5) {
            Ok(tracks) => tracks.iter().for_each(|t| println!("{} | {} | {}s | {}", t.title, t.artist, t.secs, t.url())),
            Err(e) => eprintln!("search failed: {e}"),
        },
        // `wonder play <url> [--silent]`: load it, show its state a few times, pause, quit.
        Some("play") if args.len() > 1 => {
            let extra: &[&str] = if args.iter().any(|a| a == "--silent") { &["--ao=null"] } else { &["--volume=30"] };
            let mut p = player::Player::start(extra).expect("mpv");
            assert!(p.load(&args[1]), "mpv refused the url");
            let t0 = Instant::now();
            while p.state().len == 0.0 && t0.elapsed() < Duration::from_secs(40) {
                std::thread::sleep(Duration::from_millis(300));
            }
            println!("loaded after {:.1}s: {:?}", t0.elapsed().as_secs_f32(), p.state());
            std::thread::sleep(Duration::from_secs(3));
            println!("playing: {:?}", p.state());
            p.pause(true);
            std::thread::sleep(Duration::from_millis(500));
            println!("paused: {:?}", p.state());
            p.seek(30.0);
            std::thread::sleep(Duration::from_millis(500));
            println!("after seek to 30: {:?}", p.state());
        }
        _ => eprintln!("usage: seiza-wonder search <words> | play <url> [--silent]"),
    }
}
