//! seiza-lock: the lock screen. Takes the session with ext-session-lock, shows the
//! sky, and lets go only when PAM accepts the password. If this process dies the
//! compositor keeps the session locked, so a crash never unlocks anything.
//!
//! The password service is /etc/pam.d/seiza if you have made one
//! (`auth include login` is enough), else `login`.
//! `seiza-lock --dump out.ppm [typed] [ms]` draws the frame `ms` after you typed that many characters, to a file instead.
mod ui;

use std::{
    ffi::{c_char, c_int, c_void, CString},
    sync::mpsc::{channel, Receiver},
    time::Duration,
};

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::client::{
        globals::registry_queue_init,
        protocol::{wl_callback, wl_keyboard, wl_output, wl_seat, wl_shm, wl_surface},
        Connection, Dispatch, QueueHandle,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        Capability, SeatHandler, SeatState,
    },
    session_lock::{SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface, SessionLockSurfaceConfigure},
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use ui::{Status, Ui};

// ---- password check, through PAM -------------------------------------------

#[repr(C)]
struct PamMessage {
    style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    retcode: c_int,
}

#[repr(C)]
struct PamConv {
    conv: extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int,
    data: *mut c_void,
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(service: *const c_char, user: *const c_char, conv: *const PamConv, handle: *mut *mut c_void) -> c_int;
    fn pam_authenticate(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_end(handle: *mut c_void, status: c_int) -> c_int;
}

/// PAM asks questions; the only answer we have is the password, for any prompt that wants one.
extern "C" fn answer(n: c_int, msgs: *mut *const PamMessage, out: *mut *mut PamResponse, password: *mut c_void) -> c_int {
    const PROMPT_ECHO_OFF: c_int = 1;
    const PROMPT_ECHO_ON: c_int = 2;
    const BUF_ERR: c_int = 5;
    unsafe {
        let resp = libc::calloc(n as usize, size_of::<PamResponse>()) as *mut PamResponse;
        if resp.is_null() {
            return BUF_ERR;
        }
        for i in 0..n as usize {
            let style = (**msgs.add(i)).style;
            if style == PROMPT_ECHO_OFF || style == PROMPT_ECHO_ON {
                (*resp.add(i)).resp = libc::strdup(password as *const c_char);
            }
        }
        *out = resp;
    }
    0
}

fn pam_ok(user: &str, password: &str) -> bool {
    #[cfg(debug_assertions)]
    if let Ok(t) = std::env::var("SEIZA_LOCK_TEST_PASSWORD") { // for headless tests; release builds have no such backdoor
        return t == password;
    }
    let service = if std::path::Path::new("/etc/pam.d/seiza").exists() { "seiza" } else { "login" };
    let (Ok(service), Ok(user), Ok(password)) = (CString::new(service), CString::new(user), CString::new(password)) else { return false };
    unsafe {
        let conv = PamConv { conv: answer, data: password.as_ptr() as *mut c_void };
        let mut handle = std::ptr::null_mut();
        if pam_start(service.as_ptr(), user.as_ptr(), &conv, &mut handle) != 0 {
            return false;
        }
        let rc = pam_authenticate(handle, 0);
        pam_end(handle, rc);
        rc == 0
    }
}

fn current_user() -> String {
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() { String::new() } else { std::ffi::CStr::from_ptr((*pw).pw_name).to_string_lossy().into_owned() }
    }
}

/// Overwrite a secret before dropping it, so it does not linger in freed memory.
fn wipe(s: &mut String) {
    unsafe { s.as_bytes_mut().fill(0) };
    s.clear();
}

// ---- the lock ----------------------------------------------------------------

struct Lock {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    conn: Connection,
    session_lock: Option<SessionLock>,
    surfaces: Vec<(SessionLockSurface, (u32, u32))>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    password: String,
    status: Status,
    checking: Option<Receiver<bool>>,
    shown: String,
    qh: QueueHandle<Lock>,
    frame_pending: bool, // a frame callback is out: the next redraw of an animation waits for it
    last_draw: std::time::Instant,
    user: String,
    ui: Ui,
    exit: bool,
    #[cfg(debug_assertions)]
    started: std::time::Instant,
}

impl Lock {
    fn draw(&mut self) {
        let typed = self.password.chars().count();
        self.shown = self.ui.key(typed, self.status);
        self.last_draw = std::time::Instant::now();
        for (s, (w, h)) in &self.surfaces {
            if *w == 0 {
                continue; // not configured yet: a commit now would be a protocol error
            }
            let cv = self.ui.render(*w as usize, *h as usize, typed, self.status);
            let (buffer, canvas) = self.pool.create_buffer(*w as i32, *h as i32, *w as i32 * 4, wl_shm::Format::Argb8888).expect("create buffer");
            // shortcut: assumes a little-endian host, which Argb8888 in memory then matches.
            let bytes = unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.px.len() * 4) };
            canvas[..bytes.len()].copy_from_slice(bytes);
            s.wl_surface().damage_buffer(0, 0, *w as i32, *h as i32);
            if !self.frame_pending {
                s.wl_surface().frame(&self.qh, FrameCallbackData(s.wl_surface().clone())); // animations are paced by the screen, not a timer
                self.frame_pending = true;
            }
            buffer.attach_to(s.wl_surface()).expect("buffer attach");
            s.wl_surface().commit();
        }
    }

    fn submit(&mut self) {
        if self.password.is_empty() || self.status == Status::Checking {
            return;
        }
        let (tx, rx) = channel();
        let (user, mut pw) = (self.user.clone(), std::mem::take(&mut self.password));
        std::thread::spawn(move || {
            let ok = pam_ok(&user, &pw);
            wipe(&mut pw);
            let _ = tx.send(ok);
        });
        self.checking = Some(rx);
        self.status = Status::Checking;
    }

    /// Once a tick: pick up the PAM answer, and redraw if anything on screen changed (the minute, say).
    fn tick(&mut self) {
        #[cfg(debug_assertions)]
        if let Some(ms) = std::env::var("SEIZA_LOCK_TEST_UNLOCK_MS").ok().and_then(|v| v.parse::<u128>().ok()) { // headless lock/unlock test; not in release builds
            if self.started.elapsed().as_millis() > ms && self.checking.is_none() {
                let (tx, rx) = channel();
                tx.send(true).unwrap();
                self.checking = Some(rx);
            }
        }
        match self.checking.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(true)) => {
                self.checking = None;
                if let Some(lock) = self.session_lock.take() {
                    lock.unlock();
                    let _ = self.conn.roundtrip(); // make sure the compositor has seen the unlock before we exit
                }
                self.exit = true;
                return;
            }
            Some(Ok(false)) | Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.checking = None;
                self.status = Status::Wrong;
            }
            _ => {}
        }
        if !self.surfaces.is_empty() && ((self.ui.busy() && !self.frame_pending) || self.ui.key(self.password.chars().count(), self.status) != self.shown) {
            self.draw();
        }
    }
}

impl SessionLockHandler for Lock {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {}
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        if self.session_lock.is_some() {
            eprintln!("seiza-lock: the compositor would not (or no longer will) lock the session");
        }
        self.session_lock = None;
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: SessionLockSurface, c: SessionLockSurfaceConfigure, _: u32) {
        let size = c.new_size;
        match self.surfaces.iter_mut().find(|(s, _)| s.wl_surface() == surface.wl_surface()) {
            Some(entry) => entry.1 = size,
            None => self.surfaces.push((surface, size)),
        }
        self.draw();
    }
}

impl CompositorHandler for Lock {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        if self.ui.fast() || (self.ui.busy() && self.last_draw.elapsed() >= Duration::from_millis(30)) {
            self.draw(); // the next step of whatever is animating
        } else if self.ui.busy() {
            // a slow twist does not need every frame: skip this one, but ask for the next
            if let Some((s, _)) = self.surfaces.first() {
                s.wl_surface().frame(&self.qh, FrameCallbackData(s.wl_surface().clone()));
                s.wl_surface().commit();
                self.frame_pending = true;
            }
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Lock {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl SeatHandler for Lock {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Keyboard {
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Lock {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if self.status == Status::Checking {
            return;
        }
        if self.status == Status::Wrong {
            self.status = Status::Idle;
        }
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter => self.submit(),
            Keysym::BackSpace => {
                self.password.pop();
            }
            Keysym::Escape => wipe(&mut self.password),
            _ => match event.utf8.as_deref() {
                Some(s) if !s.chars().any(char::is_control) => self.password.push_str(s),
                _ => return,
            },
        }
        self.draw();
    }
    fn repeat_key(&mut self, c: &Connection, qh: &QueueHandle<Self>, k: &wl_keyboard::WlKeyboard, serial: u32, event: KeyEvent) {
        if event.keysym == Keysym::BackSpace {
            self.press_key(c, qh, k, serial, event);
        }
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: RawModifiers, _: u32) {}
}

impl ShmHandler for Lock {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

/// A thread asks the compositor for a sync callback ten times a second; the reply wakes the
/// event loop to check the PAM answer and the clock (animation is paced by frame callbacks).
struct Tick;

impl Dispatch<wl_callback::WlCallback, Tick> for Lock {
    fn event(state: &mut Self, _: &wl_callback::WlCallback, _: wl_callback::Event, _: &Tick, _: &Connection, _: &QueueHandle<Self>) {
        state.tick();
    }
}

delegate_registry!(Lock);

impl ProvidesRegistryState for Lock {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(Lock);

fn dump(path: &str, typed: usize, ms: u64) {
    let mut ui = Ui::new();
    ui.render(1920, 1080, 0, Status::Idle);
    let mut cv = ui.render(1920, 1080, typed, Status::Idle);
    for _ in 0..ms / 40 { // let the animation run for `ms`, a frame every 40 ms, and keep the last one
        std::thread::sleep(Duration::from_millis(40));
        cv = ui.render(1920, 1080, typed, Status::Idle);
    }
    let mut ppm = format!("P6 {} {} 255\n", cv.w, cv.h).into_bytes();
    ppm.extend(cv.px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]));
    std::fs::write(path, ppm).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--dump") {
        return dump(args.get(1).expect("--dump needs a file"), args.get(2).and_then(|n| n.parse().ok()).unwrap_or(0), args.get(3).and_then(|n| n.parse().ok()).unwrap_or(0));
    }
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor missing");
    let lock_state = SessionLockState::new(&globals, &qh);
    let shm = Shm::bind(&globals, &qh).expect("wl_shm missing");

    let mut app = Lock {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        pool: SlotPool::new(4096, &shm).expect("shm pool"),
        shm,
        conn: conn.clone(),
        session_lock: None,
        surfaces: vec![],
        keyboard: None,
        password: String::new(),
        status: Status::Idle,
        checking: None,
        shown: String::new(),
        qh: qh.clone(),
        frame_pending: false,
        last_draw: std::time::Instant::now(),
        user: current_user(),
        ui: Ui::new(),
        exit: false,
        #[cfg(debug_assertions)]
        started: std::time::Instant::now(),
    };
    event_queue.roundtrip(&mut app).unwrap(); // learn the outputs before locking

    let lock = lock_state.lock(&qh).expect("the compositor has no ext-session-lock");
    // Keep the surfaces: dropping one destroys it. They get their size in `configure`.
    let pending: Vec<SessionLockSurface> = app.output_state.outputs().map(|o| lock.create_lock_surface(compositor.create_surface(&qh), &o, &qh)).collect();
    app.session_lock = Some(lock);
    app.surfaces = pending.into_iter().map(|s| (s, (0, 0))).collect();

    let (tick_conn, tick_qh) = (conn.clone(), qh.clone());
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        tick_conn.display().sync(&tick_qh, Tick);
        let _ = tick_conn.flush();
    });

    while !app.exit {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_clears_the_secret() {
        let mut s = String::from("hunter2");
        let ptr = s.as_ptr();
        wipe(&mut s);
        assert!(s.is_empty());
        assert!((0..7).all(|i| unsafe { *ptr.add(i) } == 0));
    }
}
