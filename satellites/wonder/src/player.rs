//! One mpv, started idle and kept for the whole session; the queue is loaded into it one track at a time.
use crate::ipc::{json, Ipc};
use crate::search::Track;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn prctl(option: i32, arg: u64, ...) -> i32;
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct State {
    pub pos: f64, // seconds
    pub len: f64,
    pub paused: bool,
    pub idle: bool, // nothing loaded, or the track ended
    pub title: String,
}

/// The tracks to play and which one is on. Pure, so it is tested without an mpv.
#[derive(Default)]
pub struct Queue {
    pub items: Vec<Track>,
    pub at: Option<usize>,
}

impl Queue {
    pub fn current(&self) -> Option<&Track> {
        self.at.and_then(|i| self.items.get(i))
    }

    /// Add at the end; its index.
    pub fn push(&mut self, t: Track) -> usize {
        self.items.push(t);
        self.items.len() - 1
    }

    /// Add right after the current track (or at the end with none); its index.
    pub fn insert_next(&mut self, t: Track) -> usize {
        let i = self.at.map_or(self.items.len(), |a| a + 1);
        self.items.insert(i, t);
        i
    }

    /// The index `d` places from the current one, if there is one.
    pub fn step(&self, d: isize) -> Option<usize> {
        let i = self.at? as isize + d;
        (0..self.items.len() as isize).contains(&i).then_some(i as usize)
    }

    /// Where the music goes when this track is over. Shuffle picks any other track by `roll` and never ends;
    /// repeat wraps the end back to the first.
    pub fn after(&self, shuffle: bool, repeat: bool, roll: usize) -> Option<usize> {
        let (at, n) = (self.at?, self.items.len());
        if shuffle && n > 1 {
            let i = roll % (n - 1);
            return Some(if i >= at { i + 1 } else { i });
        }
        self.step(1).or_else(|| repeat.then_some(0))
    }
}

pub struct Player {
    child: Child,
    ipc: Ipc,
    sock: PathBuf,
    pub queue: Queue,
    pub shuffle: bool,
    pub repeat: bool,
    pick: Option<usize>,     // the shuffled next track, chosen once so what is pre-fetched is what plays
    prefetched: Option<String>, // the id last sent to the cache
    pub last: State, // as of the last `poll`: what the window and MPRIS read
    ended: bool,     // the last track finished and there is no next one
    loaded_at: Instant,
}

pub type Shared = Arc<Mutex<Player>>;

impl Player {
    /// `extra` goes to mpv as is (`--ao=null` keeps a test silent).
    pub fn start(extra: &[&str]) -> std::io::Result<Player> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| "/tmp".into());
        let sock = dir.join("seiza-wonder.sock"); // one name: a killed wonder leaves a file the next start replaces
        let _ = std::fs::remove_file(&sock);
        let mut cmd = Command::new("mpv");
        cmd.args(["--idle=yes", "--no-video", "--no-terminal", "--keep-open=no", "--ytdl-format=bestaudio/best"])
            .arg(format!("--input-ipc-server={}", sock.display()))
            .args(extra)
            .stdin(Stdio::null());
        // mpv outlives a killed wonder unless the kernel tells it to follow; PDEATHSIG counts the thread that
        // spawned it, so start the player from a thread that lives as long as wonder does.
        unsafe {
            cmd.pre_exec(|| {
                prctl(1, 15); // PR_SET_PDEATHSIG, SIGTERM
                Ok(())
            });
        }
        let mut child = cmd.spawn()?;
        for _ in 0..50 {
            if let Ok(s) = UnixStream::connect(&sock) {
                let (queue, last, ended, loaded_at) = (Queue::default(), State { idle: true, ..Default::default() }, false, Instant::now());
                return Ok(Player { ipc: Ipc::new(s)?, child, sock, queue, shuffle: false, repeat: false, pick: None, prefetched: None, last, ended, loaded_at });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "mpv never opened its socket"))
    }

    /// Start one: the player, and a thread that polls it four times a second for the window and MPRIS to read.
    pub fn spawn(extra: &[&str]) -> std::io::Result<Shared> {
        let p = Arc::new(Mutex::new(Player::start(extra)?));
        let q = p.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(250));
                q.lock().unwrap().poll();
            }
        });
        Ok(p)
    }

    fn load_url(&mut self, url: &str) -> bool {
        self.ipc.ask(r#""set_property","pause",false"#); // a new track plays, even after a pause
        self.loaded_at = Instant::now();
        self.last = State { idle: false, ..Default::default() }; // loading: not idle, no length yet
        self.ipc.ask(&format!(r#""loadfile",{},"replace""#, json(url))).is_some()
    }

    /// A bare url (MPRIS OpenUri): played, but not in the queue.
    pub fn load(&mut self, url: &str) -> bool {
        self.queue.at = None;
        self.ended = false;
        self.load_url(url)
    }

    pub fn play_at(&mut self, i: usize) -> bool {
        let Some(t) = self.queue.items.get(i) else { return false };
        let url = crate::cache::audio(&t.id).map_or_else(|| t.url(), |f| f.display().to_string()); // the pre-fetched file when there is one
        self.queue.at = Some(i);
        self.pick = None;
        self.ended = false;
        self.load_url(&url)
    }

    /// Queue it at the end; it starts if nothing is playing.
    pub fn add(&mut self, t: Track) {
        let i = self.queue.push(t);
        if self.queue.at.is_none() || self.ended {
            self.play_at(i);
        }
    }

    /// Play it next, right now.
    pub fn play_now(&mut self, t: Track) {
        let i = self.queue.insert_next(t);
        self.play_at(i);
    }

    /// Where the music goes after this track: stable while it plays, so the pre-fetch and `next` agree.
    fn upcoming(&mut self) -> Option<usize> {
        if !self.shuffle {
            return self.queue.after(false, self.repeat, 0);
        }
        let roll = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
        self.pick = self.pick.filter(|i| *i < self.queue.items.len()).or_else(|| self.queue.after(true, self.repeat, roll));
        self.pick
    }

    pub fn next(&mut self) {
        match self.upcoming() {
            Some(i) => drop(self.play_at(i)),
            None => {
                self.ended = true;
                self.ipc.ask(r#""stop""#);
            }
        }
    }

    /// Back to the start of this track, or to the one before when it has only just begun.
    pub fn prev(&mut self) {
        match self.queue.step(-1) {
            Some(i) if self.last.pos < 3.0 => drop(self.play_at(i)),
            _ => drop(self.seek(0.0)),
        }
    }

    pub fn stop(&mut self) -> bool {
        self.queue.at = None;
        self.ipc.ask(r#""stop""#).is_some()
    }

    pub fn pause(&mut self, on: bool) -> bool {
        self.last.paused = on;
        self.ipc.ask(&format!(r#""set_property","pause",{on}"#)).is_some()
    }

    pub fn seek(&mut self, secs: f64) -> bool {
        self.last.pos = secs;
        self.ipc.ask(&format!(r#""seek",{secs},"absolute""#)).is_some()
    }

    fn state(&mut self) -> State {
        let mut num = |p: &str| self.ipc.ask(&format!(r#""get_property","{p}""#)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let (pos, len) = (num("time-pos"), num("duration"));
        let mut flag = |p: &str| self.ipc.ask(&format!(r#""get_property","{p}""#)).is_some_and(|v| v == "true");
        let (paused, idle) = (flag("pause"), flag("idle-active"));
        let title = self.ipc.ask(r#""get_property","media-title""#).unwrap_or_default();
        State { pos, len, paused, idle, title }
    }

    /// Read mpv's state, and go on to the next track when one has finished (or failed: a video that is gone
    /// leaves mpv idle again within a few seconds).
    pub fn poll(&mut self) {
        let s = self.state();
        let settled = self.loaded_at.elapsed() > Duration::from_secs(3);
        let over = s.idle && settled && self.queue.at.is_some() && !self.ended;
        if settled || !s.idle {
            self.last = s;
        }
        if over {
            self.next();
        } else if settled && !self.last.idle && self.last.pos > 5.0 && self.queue.at.is_some() {
            // well into a track: fetch the next one, so it starts without the wait
            let next = self.upcoming().and_then(|i| self.queue.items.get(i)).map(|t| t.id.clone());
            if let Some(id) = next.filter(|id| self.prefetched.as_ref() != Some(id)) {
                self.prefetched = Some(id.clone());
                std::thread::spawn(move || crate::cache::fetch(&id));
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.sock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str) -> Track {
        Track { id: id.into(), title: id.into(), artist: String::new(), secs: 0 }
    }

    #[test]
    fn the_queue_steps_inside_its_ends_and_inserts_after_the_current_one() {
        let mut q = Queue::default();
        assert_eq!((q.push(t("a")), q.push(t("b")), q.push(t("c"))), (0, 1, 2));
        assert_eq!(q.step(1), None, "nothing is current yet");
        q.at = Some(1);
        assert_eq!((q.step(-1), q.step(1), q.step(2), q.step(-2)), (Some(0), Some(2), None, None));
        assert_eq!(q.insert_next(t("x")), 2);
        assert_eq!(q.items.iter().map(|t| t.id.as_str()).collect::<String>(), "abxc");
        assert_eq!(q.current().unwrap().id, "b");
        q.at = None;
        assert_eq!(q.insert_next(t("y")), 4, "with none current it goes to the end");
    }

    #[test]
    fn shuffle_never_picks_the_current_track_and_repeat_wraps_the_end() {
        let mut q = Queue::default();
        (0..3).for_each(|i| { q.push(t(&i.to_string())); });
        q.at = Some(1);
        assert!((0..9).all(|r| matches!(q.after(true, false, r), Some(0 | 2))));
        q.at = Some(2);
        assert_eq!((q.after(false, false, 0), q.after(false, true, 0)), (None, Some(0)));
        assert_eq!(q.after(false, false, 0).or(q.after(false, true, 0)), Some(0));
    }
}
