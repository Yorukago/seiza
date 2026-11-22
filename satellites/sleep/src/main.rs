//! seiza-sleep: locks the screen before the machine suspends, however it was asked to (lid, power
//! button, `systemctl suspend`, idle). It holds a logind "delay" inhibitor, so the system waits for
//! the lock to be up before it goes to sleep.

use std::{process::Command, thread::sleep, time::Duration};
use zbus::{blocking::{Connection, Proxy}, zvariant::OwnedFd};

fn inhibit(logind: &Proxy) -> Option<OwnedFd> {
    logind.call("Inhibit", &("sleep", "seiza", "lock the screen before sleeping", "delay")).ok()
}

fn main() -> zbus::Result<()> {
    // One per user: it outlives a restarted session, and every copy would run the lock.
    let _only = zbus::blocking::Connection::session().and_then(|c| c.request_name_with_flags("org.seiza.Sleep", zbus::fdo::RequestNameFlags::DoNotQueue.into()).map(|_| c)); // a taken name is an error, not a place in a queue
    if let Err(e) = &_only {
        return Ok(eprintln!("seiza-sleep: already running ({e})"));
    }
    let conn = Connection::system()?;
    let logind = Proxy::new(&conn, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager")?;
    let mut hold = inhibit(&logind);
    for signal in logind.receive_signal("PrepareForSleep")? {
        if signal.body().deserialize::<bool>()? {
            if let Err(e) = Command::new("seiza-lock").spawn() {
                eprintln!("seiza-sleep: cannot run seiza-lock: {e}");
            }
            sleep(Duration::from_secs(1)); // shortcut: a fixed wait for the lock to appear, when the lock can say "I'm up" instead
            hold = None; // let the system go to sleep
        } else {
            hold = inhibit(&logind); // awake again: take the delay lock for the next sleep
        }
    }
    drop(hold);
    Ok(())
}
