//! wonder as an MPRIS player, so playerctl, the bar chip, the media keys and the widgets' media card see it.
//! shortcut: no PropertiesChanged signals and no Volume (the state it reports is the engine's last poll, up to 250 ms old); clients that poll (playerctl, the widgets) are fine,
//! ones that only listen for changes will not notice until they ask again. Add the signals when one matters.
use crate::player::{Player, Shared, State};
use crate::search::Track;
use std::collections::HashMap;
use zbus::zvariant::{ObjectPath, Value};

/// Raise asks the window to show itself again: the process outlives the window so the music keeps playing.
struct Root(std::sync::mpsc::Sender<()>);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        let _ = self.0.send(()); // nobody listening (serve mode) is fine
    }
    fn quit(&self) {}

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "wonder"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<&str> {
        vec!["http", "https"]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<&str> {
        vec![]
    }
}

struct Now(Shared);

impl Now {
    fn with<R>(&self, f: impl FnOnce(&mut Player) -> R) -> R {
        f(&mut self.0.lock().unwrap())
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Now {
    fn play(&self) {
        self.with(|p| p.pause(false));
    }
    fn pause(&self) {
        self.with(|p| p.pause(true));
    }
    fn play_pause(&self) {
        self.with(|p| p.pause(!p.last.paused));
    }
    fn stop(&self) {
        self.with(|p| p.stop());
    }
    fn next(&self) {
        self.with(|p| p.next());
    }
    fn previous(&self) {
        self.with(|p| p.prev());
    }
    fn seek(&self, offset: i64) {
        self.with(|p| {
            let at = p.last.pos + offset as f64 / 1e6;
            p.seek(at.max(0.0))
        });
    }
    fn set_position(&self, _track: ObjectPath<'_>, position: i64) {
        self.with(|p| p.seek((position as f64 / 1e6).max(0.0)));
    }
    /// A link joins the queue as a real track when yt-dlp knows it (so the window shows it), else it just plays.
    fn open_uri(&self, uri: &str) {
        match crate::search::search(uri, 1).ok().and_then(|r| r.into_iter().next()) {
            Some(t) => self.with(|p| p.play_now(t)),
            None => self.with(|p| { let _ = p.load(uri); }),
        }
    }

    #[zbus(property)]
    fn playback_status(&self) -> &'static str {
        status(&self.with(|p| p.last.clone()))
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, Value<'static>> {
        self.with(|p| metadata(&p.last, p.queue.current()))
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        (self.with(|p| p.last.pos) * 1e6) as i64
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.with(|p| p.queue.step(1).is_some())
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.with(|p| p.queue.at.is_some())
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

fn status(s: &State) -> &'static str {
    if s.idle { "Stopped" } else if s.paused { "Paused" } else { "Playing" }
}

fn metadata(s: &State, track: Option<&Track>) -> HashMap<String, Value<'static>> {
    let mut m = HashMap::new();
    if s.idle {
        m.insert("mpris:trackid".into(), Value::from(ObjectPath::from_static_str_unchecked("/org/mpris/MediaPlayer2/TrackList/NoTrack")));
        return m;
    }
    let id = track.map_or_else(|| "unknown".to_string(), |t| t.id.replace('-', "_")); // an object path has no dashes
    m.insert("mpris:trackid".into(), Value::from(ObjectPath::try_from(format!("/org/seiza/wonder/track/{id}")).unwrap_or_else(|_| ObjectPath::from_static_str_unchecked("/org/seiza/wonder/track/unknown"))));
    m.insert("mpris:length".into(), Value::from((s.len * 1e6) as i64));
    m.insert("xesam:title".into(), Value::from(track.map_or(s.title.clone(), |t| t.title.clone())));
    if let Some(t) = track {
        m.insert("xesam:artist".into(), Value::from(vec![t.artist.clone()]));
        m.insert("mpris:artUrl".into(), Value::from(t.thumbnail()));
        m.insert("xesam:url".into(), Value::from(t.url()));
    }
    m
}

/// Own `org.mpris.MediaPlayer2.wonder` for as long as the returned connection lives.
pub fn serve(p: Shared, raise: std::sync::mpsc::Sender<()>) -> zbus::Result<zbus::blocking::Connection> {
    zbus::blocking::connection::Builder::session()?
        .name("org.mpris.MediaPlayer2.wonder")?
        .serve_at("/org/mpris/MediaPlayer2", Root(raise))?
        .serve_at("/org/mpris/MediaPlayer2", Now(p))?
        .build()
}

/// Ask a running wonder to show its window; false when none is running.
pub fn raise_running() -> bool {
    zbus::blocking::Connection::session()
        .and_then(|c| c.call_method(Some("org.mpris.MediaPlayer2.wonder"), "/org/mpris/MediaPlayer2", Some("org.mpris.MediaPlayer2"), "Raise", &()))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_follows_idle_then_paused() {
        let s = |idle, paused| State { idle, paused, ..Default::default() };
        assert_eq!(status(&s(true, false)), "Stopped");
        assert_eq!(status(&s(false, true)), "Paused");
        assert_eq!(status(&s(false, false)), "Playing");
    }

    #[test]
    fn metadata_uses_the_track_and_an_idle_player_has_none() {
        let s = State { len: 226.0, title: "mpv title".into(), ..Default::default() };
        let t = Track { id: "a-b_c".into(), title: "Song".into(), artist: "Band".into(), secs: 226 };
        let m = metadata(&s, Some(&t));
        assert_eq!(m["xesam:title"], Value::from("Song".to_string()));
        assert_eq!(m["mpris:length"], Value::from(226_000_000i64));
        assert_eq!(m["mpris:trackid"], Value::from(ObjectPath::try_from("/org/seiza/wonder/track/a_b_c").unwrap()));
        assert_eq!(metadata(&s, None)["xesam:title"], Value::from("mpv title".to_string()));
        assert_eq!(metadata(&State { idle: true, ..Default::default() }, None).len(), 1);
    }
}
