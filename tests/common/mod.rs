//! Interop test harness: a private dbus-daemon, the real protocol core on its
//! own thread with a scripted UI, and gcr's own client (`GcrSystemPrompt`, what
//! gnome-keyring and seahorse use) over FFI. No display, no GTK.
//! docs/skills/protocol.md § Interop tests.

#![allow(dead_code)]

use std::{
    collections::VecDeque,
    ffi::{CStr, CString, c_char, c_int, c_void},
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use gtk::glib;
use krytis_prompter::{
    daemon,
    ui::{Answer, PromptRequest, PromptUi, Reply},
};
use zeroize::Zeroizing;

/// A private session bus, killed on drop.
pub struct Bus {
    pub address: String,
    child: Child,
    dir: PathBuf,
}

impl Bus {
    pub fn start() -> Self {
        let dir = std::env::temp_dir().join(format!("krytis-prompter-test-{}-{}", std::process::id(), unique()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>"#,
                dir.display()
            ),
        )
        .unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon must be on PATH");
        let mut address = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).unwrap();
        Self { address: address.trim().to_owned(), child, dir }
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// What the scripted UI answers to the next prompt.
pub enum Script {
    Password(&'static str),
    PasswordChoice(&'static str, bool),
    Confirm {
        choice_chosen: bool,
    },
    Cancel,
    /// Leave the prompt open with no answer.
    Hold,
}

/// What the scripted UI was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    Prompt(Box<PromptRequest>),
    Close,
}

#[derive(Clone, Default)]
pub struct Ui {
    script: Arc<Mutex<VecDeque<Script>>>,
    events: Arc<Mutex<Vec<UiEvent>>>,
}

impl Ui {
    pub fn answer(&self, script: Script) {
        self.script.lock().unwrap().push_back(script);
    }

    pub fn events(&self) -> Vec<UiEvent> {
        self.events.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.events.lock().unwrap().clear();
    }

    /// Waits until the UI has seen at least `n` events.
    pub fn wait_events(&self, n: usize) -> Vec<UiEvent> {
        for _ in 0..200 {
            let events = self.events();
            if events.len() >= n {
                return events;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!("UI saw only {:?}, wanted {n} events", self.events());
    }
}

/// The scripted UI as the daemon's thread sees it.
struct ScriptedUi(Ui);

impl PromptUi for ScriptedUi {
    fn prompt(&self, request: &PromptRequest, reply: Reply) {
        self.0.events.lock().unwrap().push(UiEvent::Prompt(Box::new(request.clone())));
        let answer = match self.0.script.lock().unwrap().pop_front().unwrap_or(Script::Hold) {
            Script::Password(p) => Answer::Password { secret: Zeroizing::new(p.to_owned()), choice_chosen: false },
            Script::PasswordChoice(p, c) => Answer::Password { secret: Zeroizing::new(p.to_owned()), choice_chosen: c },
            Script::Confirm { choice_chosen } => Answer::Confirm { choice_chosen },
            Script::Cancel => Answer::Cancel,
            Script::Hold => return,
        };
        // Schedule on the daemon's thread-default context, after prompt()
        // returns. GLib's idle_add_local_once targets the global default.
        glib::MainContext::ref_thread_default().spawn_local(async move { reply(answer) });
    }

    fn close(&self) {
        self.0.events.lock().unwrap().push(UiEvent::Close);
    }
}

/// The protocol core serving `bus` on its own thread. `done` receives the
/// result of `Prompter::run` when it returns (idle exit).
pub struct Daemon {
    pub ui: Ui,
    pub done: mpsc::Receiver<Result<(), String>>,
}

pub fn start_daemon(bus: &Bus, idle_timeout: Option<Duration>) -> Daemon {
    let ui = Ui::default();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (done_tx, done) = mpsc::channel();
    let address = bus.address.clone();
    let thread_ui = ui.clone();
    thread::spawn(move || {
        let ctx = glib::MainContext::new();
        let result = ctx
            .with_thread_default(|| {
                ctx.block_on(async move {
                    let conn = zbus::connection::Builder::address(address.as_str()).build().await?;
                    let prompter = daemon::Prompter::new(conn).await?;
                    assert!(prompter.claim_name().await?, "the prompter name was taken");
                    ready_tx.send(()).unwrap();
                    prompter.run(Rc::new(ScriptedUi(thread_ui)), daemon::Config { idle_timeout }).await
                })
            })
            .expect("thread-default context");
        let _ = done_tx.send(result.map_err(|e| e.to_string()));
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).expect("daemon did not start");
    Daemon { ui, done }
}

/// A daemon held after connecting and before claiming the name, until `go`.
/// That is the window D-Bus activation creates: the bus starts the process,
/// and the call that activated it is already waiting when the name is claimed.
pub struct HeldDaemon {
    pub daemon: Daemon,
    /// The daemon connection's unique name, to call before it owns the name.
    pub unique_name: String,
    pub go: mpsc::Sender<()>,
}

pub fn start_daemon_held(bus: &Bus) -> HeldDaemon {
    let ui = Ui::default();
    let (name_tx, name_rx) = mpsc::channel();
    let (go, go_rx) = mpsc::channel::<()>();
    let (done_tx, done) = mpsc::channel();
    let address = bus.address.clone();
    let thread_ui = ui.clone();
    thread::spawn(move || {
        let ctx = glib::MainContext::new();
        let result = ctx
            .with_thread_default(|| {
                ctx.block_on(async move {
                    let conn = zbus::connection::Builder::address(address.as_str()).build().await?;
                    let unique_name = conn.unique_name().unwrap().to_string();
                    let prompter = daemon::Prompter::new(conn).await?;
                    name_tx.send(unique_name).unwrap();
                    go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    assert!(prompter.claim_name().await?, "the prompter name was taken");
                    prompter.run(Rc::new(ScriptedUi(thread_ui)), daemon::Config { idle_timeout: None }).await
                })
            })
            .expect("thread-default context");
        let _ = done_tx.send(result.map_err(|e| e.to_string()));
    });
    let unique_name = name_rx.recv_timeout(Duration::from_secs(10)).expect("daemon did not connect");
    HeldDaemon { daemon: Daemon { ui, done }, unique_name, go }
}

/// Points this process's gcr client at `bus`. Call once, before any gcr or GIO
/// call: GIO reads the address when it first connects.
pub fn use_bus_for_gcr(bus: &Bus) {
    // SAFETY: called at the start of a single-test binary, before any other
    // thread reads the environment.
    unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &bus.address) };
}

// ---- gcr's client, GcrSystemPrompt ----------------------------------------

#[repr(C)]
struct GError {
    domain: u32,
    code: c_int,
    message: *const c_char,
}

#[link(name = "gcr-4")]
unsafe extern "C" {
    fn gcr_system_prompt_open(timeout_seconds: c_int, cancellable: *mut c_void, error: *mut *mut GError)
    -> *mut c_void;
    fn gcr_system_prompt_close(prompt: *mut c_void, cancellable: *mut c_void, error: *mut *mut GError) -> c_int;
    fn gcr_prompt_set_title(prompt: *mut c_void, value: *const c_char);
    fn gcr_prompt_set_message(prompt: *mut c_void, value: *const c_char);
    fn gcr_prompt_set_warning(prompt: *mut c_void, value: *const c_char);
    fn gcr_prompt_set_choice_label(prompt: *mut c_void, value: *const c_char);
    fn gcr_prompt_set_password_new(prompt: *mut c_void, value: c_int);
    fn gcr_prompt_get_choice_chosen(prompt: *mut c_void) -> c_int;
    fn gcr_prompt_get_password_strength(prompt: *mut c_void) -> c_int;
    fn gcr_prompt_password(prompt: *mut c_void, cancellable: *mut c_void, error: *mut *mut GError) -> *const c_char;
    fn gcr_prompt_confirm(prompt: *mut c_void, cancellable: *mut c_void, error: *mut *mut GError) -> c_int;
    fn g_error_free(error: *mut GError);
    fn g_object_unref(object: *mut c_void);
}

fn take_error(error: *mut GError) -> String {
    // SAFETY: a GError gcr just set.
    let message = unsafe { CStr::from_ptr((*error).message) }.to_string_lossy().into_owned();
    unsafe { g_error_free(error) };
    message
}

/// gcr's client side of one prompting session. Its calls block, running
/// their own main loop, like a gnome-keyring prompt does.
pub struct GcrPrompt(*mut c_void);

impl GcrPrompt {
    pub fn open(timeout_seconds: i32) -> Result<Self, String> {
        let mut error = std::ptr::null_mut();
        // SAFETY: plain blocking call.
        let prompt = unsafe { gcr_system_prompt_open(timeout_seconds, std::ptr::null_mut(), &mut error) };
        if prompt.is_null() { Err(take_error(error)) } else { Ok(Self(prompt)) }
    }

    fn set(&self, f: unsafe extern "C" fn(*mut c_void, *const c_char), value: &str) {
        let value = CString::new(value).unwrap();
        // SAFETY: gcr copies the string.
        unsafe { f(self.0, value.as_ptr()) }
    }

    pub fn title(&self, v: &str) {
        self.set(gcr_prompt_set_title, v);
    }
    pub fn message(&self, v: &str) {
        self.set(gcr_prompt_set_message, v);
    }
    pub fn warning(&self, v: &str) {
        self.set(gcr_prompt_set_warning, v);
    }
    pub fn choice_label(&self, v: &str) {
        self.set(gcr_prompt_set_choice_label, v);
    }
    pub fn password_new(&self, v: bool) {
        // SAFETY: plain setter.
        unsafe { gcr_prompt_set_password_new(self.0, c_int::from(v)) }
    }
    pub fn choice_chosen(&self) -> bool {
        // SAFETY: plain getter.
        unsafe { gcr_prompt_get_choice_chosen(self.0) != 0 }
    }
    pub fn password_strength(&self) -> i32 {
        // SAFETY: plain getter.
        unsafe { gcr_prompt_get_password_strength(self.0) }
    }

    /// `Ok(None)` is a cancelled prompt.
    pub fn password(&self) -> Result<Option<Vec<u8>>, String> {
        let mut error = std::ptr::null_mut();
        // SAFETY: blocking call; the result is owned by the prompt, copied here.
        let secret = unsafe { gcr_prompt_password(self.0, std::ptr::null_mut(), &mut error) };
        if !error.is_null() {
            return Err(take_error(error));
        }
        Ok((!secret.is_null()).then(|| unsafe { CStr::from_ptr(secret) }.to_bytes().to_vec()))
    }

    /// `true` for Continue, `false` for Cancel.
    pub fn confirm(&self) -> Result<bool, String> {
        let mut error = std::ptr::null_mut();
        // SAFETY: blocking call.
        let reply = unsafe { gcr_prompt_confirm(self.0, std::ptr::null_mut(), &mut error) };
        if !error.is_null() {
            return Err(take_error(error));
        }
        Ok(reply == 1) // GCR_PROMPT_REPLY_CONTINUE
    }

    pub fn close(self) -> Result<(), String> {
        let mut error = std::ptr::null_mut();
        // SAFETY: blocking call, then drop our reference.
        let ok = unsafe { gcr_system_prompt_close(self.0, std::ptr::null_mut(), &mut error) };
        unsafe { g_object_unref(self.0) };
        std::mem::forget(self);
        if ok == 0 { Err(take_error(error)) } else { Ok(()) }
    }
}

impl Drop for GcrPrompt {
    fn drop(&mut self) {
        // SAFETY: our reference.
        unsafe { g_object_unref(self.0) }
    }
}
