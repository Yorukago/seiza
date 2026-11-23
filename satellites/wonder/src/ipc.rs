//! mpv's JSON IPC: one request per line, answers matched by `request_id`, events in between skipped.
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub struct Ipc {
    w: UnixStream,
    r: BufReader<UnixStream>,
    id: u64,
}

impl Ipc {
    pub fn new(s: UnixStream) -> std::io::Result<Ipc> {
        s.set_read_timeout(Some(Duration::from_secs(2)))?;
        Ok(Ipc { r: BufReader::new(s.try_clone()?), w: s, id: 0 })
    }

    /// Run a command whose items are already JSON (`"get_property","pause"`). Its answer's data as text,
    /// empty when the command succeeded with no data, None when it failed or mpv went quiet.
    pub fn ask(&mut self, items: &str) -> Option<String> {
        self.id += 1;
        writeln!(self.w, r#"{{"command":[{items}],"request_id":{}}}"#, self.id).ok()?;
        let tag = format!(r#""request_id":{},"#, self.id);
        loop {
            let mut line = String::new();
            if self.r.read_line(&mut line).ok()? == 0 {
                return None;
            }
            if line.contains(&tag) {
                return reply_data(&line);
            }
        }
    }
}

/// `{"data":4.3,"request_id":1,"error":"success"}` -> `4.3`; a string comes back unquoted.
fn reply_data(line: &str) -> Option<String> {
    if !line.contains(r#""error":"success""#) {
        return None;
    }
    let Some(rest) = line.strip_prefix(r#"{"data":"#) else { return Some(String::new()) };
    let data = rest.split_once(r#","request_id""#)?.0;
    // shortcut: only the escapes mpv uses for text; a \u00XX control character stays as written.
    Some(match data.strip_prefix('"').and_then(|d| d.strip_suffix('"')) {
        Some(s) => s.replace("\\\"", "\"").replace("\\/", "/").replace("\\n", " ").replace("\\\\", "\\"),
        None => data.to_string(),
    })
}

/// A string as a JSON literal.
pub fn json(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_by_request_id_and_events_are_skipped() {
        let (a, b) = UnixStream::pair().unwrap();
        let server = std::thread::spawn(move || {
            let mut r = BufReader::new(b.try_clone().unwrap());
            let mut w = b;
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            assert_eq!(line, "{\"command\":[\"get_property\",\"time-pos\"],\"request_id\":1}\n");
            w.write_all(b"{\"event\":\"playback-restart\"}\n{\"data\":4.5,\"request_id\":1,\"error\":\"success\"}\n").unwrap();
            line.clear();
            r.read_line(&mut line).unwrap();
            w.write_all(b"{\"request_id\":2,\"error\":\"property unavailable\"}\n").unwrap();
            line.clear();
            r.read_line(&mut line).unwrap();
            w.write_all(b"{\"data\":\"a \\\"b\\\" c\",\"request_id\":3,\"error\":\"success\"}\n{\"request_id\":4,\"error\":\"success\"}\n").unwrap();
            line.clear();
            r.read_line(&mut line).unwrap();
        });
        let mut ipc = Ipc::new(a).unwrap();
        assert_eq!(ipc.ask(r#""get_property","time-pos""#).as_deref(), Some("4.5"));
        assert_eq!(ipc.ask(r#""get_property","duration""#), None, "an error is None");
        assert_eq!(ipc.ask(r#""get_property","media-title""#).as_deref(), Some("a \"b\" c"));
        assert_eq!(ipc.ask(r#""set_property","pause",true"#).as_deref(), Some(""), "success without data");
        server.join().unwrap();
    }

    #[test]
    fn json_quotes_a_string() {
        assert_eq!(json(r#"a"b\c"#), r#""a\"b\\c""#);
    }
}
