//! One mpv, started idle and kept for the whole session; tracks are loaded into it.
use crate::ipc::{json, Ipc};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Debug, Default, PartialEq)]
pub struct State {
    pub pos: f64, // seconds
    pub len: f64,
    pub paused: bool,
    pub idle: bool, // nothing loaded, or the track ended
    pub title: String,
}

pub struct Player {
    child: Child,
    ipc: Ipc,
    sock: PathBuf,
}

impl Player {
    /// `extra` goes to mpv as is (`--ao=null` keeps a test silent).
    pub fn start(extra: &[&str]) -> std::io::Result<Player> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| "/tmp".into());
        let sock = dir.join(format!("seiza-wonder-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock);
        let mut child = Command::new("mpv")
            .args(["--idle=yes", "--no-video", "--no-terminal", "--ytdl-format=bestaudio/best"])
            .arg(format!("--input-ipc-server={}", sock.display()))
            .args(extra)
            .stdin(Stdio::null())
            .spawn()?;
        for _ in 0..50 {
            if let Ok(s) = UnixStream::connect(&sock) {
                return Ok(Player { ipc: Ipc::new(s)?, child, sock });
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "mpv never opened its socket"))
    }

    pub fn load(&mut self, url: &str) -> bool {
        self.ipc.ask(&format!(r#""loadfile",{},"replace""#, json(url))).is_some()
    }

    pub fn pause(&mut self, on: bool) -> bool {
        self.ipc.ask(&format!(r#""set_property","pause",{on}"#)).is_some()
    }

    pub fn seek(&mut self, secs: f64) -> bool {
        self.ipc.ask(&format!(r#""seek",{secs},"absolute""#)).is_some()
    }

    pub fn state(&mut self) -> State {
        let mut num = |p: &str| self.ipc.ask(&format!(r#""get_property","{p}""#)).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let (pos, len) = (num("time-pos"), num("duration"));
        let mut flag = |p: &str| self.ipc.ask(&format!(r#""get_property","{p}""#)).is_some_and(|v| v == "true");
        let (paused, idle) = (flag("pause"), flag("idle-active"));
        let title = self.ipc.ask(r#""get_property","media-title""#).unwrap_or_default();
        State { pos, len, paused, idle, title }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.sock);
    }
}
