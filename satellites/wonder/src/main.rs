//! wonder: the Seiza music player. For now only its engine (mpv + yt-dlp) and a terminal harness for it;
//! the window comes later.
mod ipc;
mod lyrics;
mod mpris;
mod player;
mod search;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("search") => match search::search(&args[1..].join(" "), 5) {
            Ok(tracks) => tracks.iter().for_each(|t| println!("{} | {} | {}s | {}", t.title, t.artist, t.secs, t.url())),
            Err(e) => eprintln!("search failed: {e}"),
        },
        // `wonder lyrics <words>`: the lyrics of the first search result, to see what the window would show.
        Some("lyrics") => {
            let Some(t) = search::search(&args[1..].join(" "), 1).ok().and_then(|r| r.into_iter().next()) else { return eprintln!("no such track") };
            println!("{} ({}s)", t.title, t.secs);
            match lyrics::fetch(&t) {
                Some(l) => println!("{} lines, synced: {}\n{:?}", l.lines.len(), l.synced, &l.lines[..l.lines.len().min(4)]),
                None => println!("no lyrics"),
            }
        }
        // `wonder serve [--silent] [words]`: a player on the session bus; `playerctl -p wonder open <url>` loads a track.
        Some("serve") => {
            let extra: &[&str] = if args.iter().any(|a| a == "--silent") { &["--ao=null"] } else { &["--volume=30"] };
            let player = player::Player::spawn(extra).expect("mpv");
            let query = args[1..].iter().filter(|a| *a != "--silent").cloned().collect::<Vec<_>>().join(" ");
            if !query.is_empty() {
                for t in search::search(&query, 3).unwrap_or_default() {
                    player.lock().unwrap().add(t); // the first results of any words after the flags
                }
            }
            let _bus = mpris::serve(player).expect("the MPRIS name is taken or there is no session bus");
            loop {
                std::thread::park();
            }
        }
        _ => eprintln!("usage: seiza-wonder search <words> | lyrics <words> | serve [--silent] [words]"),
    }
}
