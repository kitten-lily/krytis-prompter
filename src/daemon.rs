//! The `org.gnome.keyring.internal.Prompter` service, in single-prompt mode.
//!
//! This follows gcr's own `GcrSystemPrompter` (`gcr/gcr-system-prompter.c`) with
//! `GCR_SYSTEM_PROMPTER_SINGLE`: one caller's session is active at a time and
//! the rest wait in order. Per session:
//!
//! 1. `BeginPrompting(callback)`: queue; when it's this session's turn, call the
//!    caller's `PromptReady("", {}, <our public key>)`.
//! 2. `PerformPrompt(callback, type, props, exchange)`: show the prompt; on the
//!    answer call `PromptReady("yes"|"no", <changed props>, <exchange>)`. A
//!    session may prompt again (a wrong password does).
//! 3. `StopPrompting(callback)`: close, call `PromptDone()`, next session.
//!
//! **Reply ordering is load-bearing.** gcr's client arms its wait for
//! `PromptReady` only once `BeginPrompting` returns; a `PromptReady` that
//! overtakes the reply kills the session ("Another prompt is already in
//! progress"). Method calls are therefore read from a raw [`MessageStream`] and
//! each reply is awaited before anything it triggers is sent. zbus's
//! `#[interface]` replies only when the handler returns, so it can't promise
//! that. docs/skills/protocol.md § Reply ordering.

use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::{Rc, Weak},
    time::Duration,
};

use futures_channel::mpsc;
use futures_util::{StreamExt, stream};
use gtk::glib;
use zbus::{
    Connection, Message, MessageStream, OwnedObjectPath, OwnedVariant, Variant,
    fdo::{DBusProxy, ReleaseNameReply, RequestNameFlags, RequestNameReply},
    message::{Flags, Type as MessageType},
};

use crate::exchange::Exchange;
use crate::ui::{Answer, PromptKind, PromptRequest, PromptUi, Props, password_strength};

pub const BUS_NAME: &str = "org.gnome.keyring.SystemPrompter";
pub const PATH: &str = "/org/gnome/keyring/Prompter";
pub const INTERFACE: &str = "org.gnome.keyring.internal.Prompter";
pub const CALLBACK_INTERFACE: &str = "org.gnome.keyring.internal.Prompter.Callback";

const INTROSPECTION: &str = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.gnome.keyring.internal.Prompter">
    <method name="BeginPrompting"><arg name="callback" type="o" direction="in"/></method>
    <method name="PerformPrompt">
      <arg name="callback" type="o" direction="in"/>
      <arg name="type" type="s" direction="in"/>
      <arg name="properties" type="a{sv}" direction="in"/>
      <arg name="exchange" type="s" direction="in"/>
    </method>
    <method name="StopPrompting"><arg name="callback" type="o" direction="in"/></method>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect"><arg name="xml_data" type="s" direction="out"/></method>
  </interface>
  <interface name="org.freedesktop.DBus.Peer">
    <method name="Ping"/>
  </interface>
</node>
"#;

/// How long the prompter lingers with no session before exiting.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// `None` never exits.
    pub idle_timeout: Option<Duration>,
}

/// Claims [`BUS_NAME`] without queueing. `false` means another prompter
/// (gnome-shell, a live `gcr-prompter`, noctalia's) already owns it, and this
/// one should stay out of the way.
pub async fn claim_name(conn: &Connection) -> zbus::Result<bool> {
    let reply = DBusProxy::new(conn).await?.request_name(BUS_NAME.try_into()?, RequestNameFlags::DO_NOT_QUEUE).await?;
    Ok(matches!(reply, RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner))
}

/// A caller's session: its unique bus name and its callback object.
type Key = (String, String);

#[derive(PartialEq, Eq)]
enum Phase {
    /// `PromptReady` sent; waiting for `PerformPrompt` or `StopPrompting`.
    Ready,
    /// The UI is showing a prompt and owes an answer.
    Prompting,
}

struct Active {
    key: Key,
    /// Distinguishes this session from a later one with the same key, so a late
    /// UI answer or a failed `PromptReady` never touches the wrong session.
    generation: u64,
    exchange: Exchange,
    props: Props,
    kind: PromptKind,
    phase: Phase,
}

struct State {
    conn: Connection,
    ui: Rc<dyn PromptUi>,
    config: Config,
    waiting: VecDeque<Key>,
    active: Option<Active>,
    generation: u64,
    idle_source: Option<glib::JoinHandle<()>>,
    /// Set once the name is released on the way out.
    exiting: bool,
    events: mpsc::UnboundedSender<Event>,
}

enum Event {
    Message(zbus::Result<Message>),
    CallerGone(String),
    Idle,
    Exit,
}

type Shared = Rc<RefCell<State>>;

/// Serves the prompter on `conn` until it has been idle for
/// `config.idle_timeout`. Claim the name first with [`claim_name`].
pub async fn run(conn: Connection, ui: Rc<dyn PromptUi>, config: Config) -> zbus::Result<()> {
    let messages = MessageStream::from(&conn).map(Event::Message);
    let gone = DBusProxy::new(&conn).await?.receive_name_owner_changed().await?.filter_map(|signal| async move {
        let args = signal.args().ok()?;
        // A unique name losing its owner is a client disconnecting.
        (args.name().starts_with(':') && args.new_owner().is_none()).then(|| Event::CallerGone(args.name().to_string()))
    });
    let (events, internal) = mpsc::unbounded();
    let events_in = stream::select(stream::select(messages, gone), internal);
    futures_util::pin_mut!(events_in);
    let state: Shared = Rc::new(RefCell::new(State {
        conn: conn.clone(),
        ui,
        config,
        waiting: VecDeque::new(),
        active: None,
        generation: 0,
        idle_source: None,
        exiting: false,
        events,
    }));
    update_idle(&state);

    while let Some(event) = events_in.next().await {
        match event {
            Event::Message(Ok(msg)) => handle_message(&state, &msg).await,
            Event::Message(Err(e)) => log(&format!("bus error: {e}")),
            Event::CallerGone(name) => caller_gone(&state, &name),
            Event::Idle => {
                if !is_idle(&state) {
                    continue;
                }
                // Release first, then drain what was already routed to us: new
                // callers reach a freshly activated instance instead.
                let reply = DBusProxy::new(&conn).await?.release_name(BUS_NAME.try_into()?).await?;
                if reply != ReleaseNameReply::Released {
                    log(&format!("ReleaseName: {reply:?}"));
                }
                state.borrow_mut().exiting = true;
                let tx = state.borrow().events.clone();
                glib::MainContext::ref_thread_default().spawn_local(async move {
                    glib::timeout_future(Duration::from_millis(250)).await;
                    let _ = tx.unbounded_send(Event::Exit);
                });
            }
            Event::Exit => {
                if is_idle(&state) {
                    break;
                }
            }
        }
    }
    Ok(())
}

fn is_idle(state: &Shared) -> bool {
    let s = state.borrow();
    s.active.is_none() && s.waiting.is_empty()
}

/// Arms the idle timer while no session exists, and disarms it otherwise, so
/// the daemon never exits mid-prompt.
fn update_idle(state: &Shared) {
    let idle = is_idle(state);
    let mut s = state.borrow_mut();
    match (idle, s.config.idle_timeout, s.idle_source.is_some()) {
        (true, Some(timeout), false) if !s.exiting => {
            let tx = s.events.clone();
            let weak = Rc::downgrade(state);
            let handle = glib::MainContext::ref_thread_default().spawn_local(async move {
                glib::timeout_future(timeout).await;
                if let Some(state) = weak.upgrade() {
                    state.borrow_mut().idle_source = None;
                }
                let _ = tx.unbounded_send(Event::Idle);
            });
            s.idle_source = Some(handle);
        }
        (false, _, true) => {
            if let Some(handle) = s.idle_source.take() {
                handle.abort();
            }
        }
        _ => {}
    }
}

async fn handle_message(state: &Shared, msg: &Message) {
    let hdr = msg.header();
    if hdr.message_type() != MessageType::MethodCall {
        return;
    }
    let conn = state.borrow().conn.clone();
    let wants_reply = !hdr.primary().flags().contains(Flags::NO_REPLY_EXPECTED);
    let path = hdr.path().map(|p| p.as_str()).unwrap_or_default();
    let interface = hdr.interface().map(|i| i.as_str());
    let member = hdr.member().map(|m| m.as_str()).unwrap_or_default();
    let sender = hdr.sender().map(|s| s.to_string()).unwrap_or_default();

    let result = match (interface, member) {
        (Some("org.freedesktop.DBus.Peer"), "Ping") => {
            if wants_reply {
                let _ = conn.reply(&hdr, &()).await;
            }
            return;
        }
        (Some("org.freedesktop.DBus.Introspectable"), "Introspect") if path == PATH => {
            if wants_reply {
                let _ = conn.reply(&hdr, &INTROSPECTION).await;
            }
            return;
        }
        (Some(INTERFACE) | None, "BeginPrompting") if path == PATH => begin_prompting(state, &conn, msg, sender).await,
        (Some(INTERFACE) | None, "PerformPrompt") if path == PATH => perform_prompt(state, &conn, msg, sender).await,
        (Some(INTERFACE) | None, "StopPrompting") if path == PATH => stop_prompting(state, &conn, msg, sender).await,
        _ => Err(MethodError::UnknownMethod(format!("No method {member} on {path}"))),
    };
    if let Err(e) = result
        && wants_reply
    {
        let (name, text) = e.parts();
        let _ = conn.reply_error(&hdr, name, &text).await;
    }
    update_idle(state);
}

/// An error a method returns to its caller. Success replies are sent by the
/// handlers themselves, because what follows a reply must wait for it.
enum MethodError {
    Failed(String),
    InvalidArgs(String),
    UnknownMethod(String),
}

impl MethodError {
    fn parts(&self) -> (&'static str, String) {
        match self {
            Self::Failed(m) => ("org.freedesktop.DBus.Error.Failed", m.clone()),
            Self::InvalidArgs(m) => ("org.freedesktop.DBus.Error.InvalidArgs", m.clone()),
            Self::UnknownMethod(m) => ("org.freedesktop.DBus.Error.UnknownMethod", m.clone()),
        }
    }
}

fn invalid_args(e: zbus::Error) -> MethodError {
    MethodError::InvalidArgs(e.to_string())
}

async fn begin_prompting(state: &Shared, conn: &Connection, msg: &Message, sender: String) -> Result<(), MethodError> {
    let callback: OwnedObjectPath = msg.body().deserialize().map_err(invalid_args)?;
    let key = (sender, callback.to_string());
    {
        let s = state.borrow();
        if s.exiting {
            // We no longer own the name, so this caller's PerformPrompt would
            // reach the next instance, which has never heard of it.
            return Err(MethodError::Failed("The prompter is exiting; try again".into()));
        }
        if s.waiting.contains(&key) || s.active.as_ref().is_some_and(|a| a.key == key) {
            return Err(MethodError::Failed("Already begun prompting for this prompt callback".into()));
        }
    }
    state.borrow_mut().waiting.push_back(key);
    let _ = conn.reply(&msg.header(), &()).await;
    update_idle(state);
    next_ready(state);
    Ok(())
}

async fn perform_prompt(state: &Shared, conn: &Connection, msg: &Message, sender: String) -> Result<(), MethodError> {
    let (callback, kind, changed, exchange): (OwnedObjectPath, String, HashMap<String, OwnedVariant>, String) =
        msg.body().deserialize().map_err(invalid_args)?;
    let key = (sender, callback.to_string());
    let (generation, request) = {
        let mut s = state.borrow_mut();
        let Some(active) = s.active.as_mut().filter(|a| a.key == key) else {
            return Err(MethodError::Failed("Not begun prompting for this prompt callback".into()));
        };
        if active.phase == Phase::Prompting {
            return Err(MethodError::Failed("Already performing a prompt for this prompt callback".into()));
        }
        let kind = match kind.as_str() {
            "password" => PromptKind::Password,
            "confirm" => PromptKind::Confirm,
            _ => return Err(MethodError::InvalidArgs("Invalid type argument".into())),
        };
        if !active.exchange.receive(&exchange) {
            return Err(MethodError::InvalidArgs("Invalid secret exchange received".into()));
        }
        active.props.update(&changed);
        active.kind = kind;
        active.phase = Phase::Prompting;
        (active.generation, PromptRequest { kind, props: active.props.clone() })
    };
    let _ = conn.reply(&msg.header(), &()).await;

    // The session may have ended while the reply was in flight.
    let ui = {
        let s = state.borrow();
        if !s.active.as_ref().is_some_and(|a| a.generation == generation) {
            return Ok(());
        }
        s.ui.clone()
    };
    let weak = Rc::downgrade(state);
    ui.prompt(&request, Box::new(move |answer| answered(&weak, generation, answer)));
    Ok(())
}

async fn stop_prompting(state: &Shared, conn: &Connection, msg: &Message, sender: String) -> Result<(), MethodError> {
    let callback: OwnedObjectPath = msg.body().deserialize().map_err(invalid_args)?;
    let key = (sender, callback.to_string());
    let was_active = end_session(state, &key);
    if was_active {
        // As gcr does: PromptDone goes out without waiting for its reply, before
        // StopPrompting returns.
        if let Err(e) = send_prompt_done(conn, &key).await {
            log(&format!("PromptDone to {}: {e}", key.0));
        }
    }
    let _ = conn.reply(&msg.header(), &()).await;
    next_ready(state);
    Ok(())
}

async fn send_prompt_done(conn: &Connection, key: &Key) -> zbus::Result<()> {
    let done = Message::method_call(key.1.as_str(), "PromptDone")
        .destination(key.0.as_str())
        .interface(CALLBACK_INTERFACE)
        .build(&())?;
    conn.send(&done).await
}

/// Removes `key` from the queue or the active slot. Returns whether it was
/// the active session (whose UI was then closed).
fn end_session(state: &Shared, key: &Key) -> bool {
    let ui = {
        let mut s = state.borrow_mut();
        s.waiting.retain(|k| k != key);
        if !s.active.as_ref().is_some_and(|a| &a.key == key) {
            return false;
        }
        s.active = None;
        s.ui.clone()
    };
    ui.close();
    true
}

fn caller_gone(state: &Shared, name: &str) {
    let keys: Vec<Key> = {
        let s = state.borrow();
        s.waiting.iter().chain(s.active.as_ref().map(|a| &a.key)).filter(|k| k.0 == name).cloned().collect()
    };
    if keys.is_empty() {
        return;
    }
    for key in &keys {
        // No PromptDone: there's nobody left to receive it.
        end_session(state, key);
    }
    next_ready(state);
    update_idle(state);
}

/// Starts the next waiting session if the active slot is free.
fn next_ready(state: &Shared) {
    let (generation, key, begin) = {
        let mut s = state.borrow_mut();
        if s.active.is_some() {
            return;
        }
        let Some(key) = s.waiting.pop_front() else { return };
        s.generation += 1;
        let generation = s.generation;
        let exchange = Exchange::new();
        let begin = exchange.begin();
        s.active = Some(Active {
            key: key.clone(),
            generation,
            exchange,
            props: Props::default(),
            kind: PromptKind::Password,
            phase: Phase::Ready,
        });
        (generation, key, begin)
    };
    send_ready(state, generation, key, "", HashMap::new(), begin);
}

fn answered(state: &Weak<RefCell<State>>, generation: u64, answer: Answer) {
    let Some(state) = state.upgrade() else { return };
    let (key, reply, props, exchange) = {
        let mut s = state.borrow_mut();
        let Some(active) = s.active.as_mut().filter(|a| a.generation == generation && a.phase == Phase::Prompting)
        else {
            return; // The session ended first; the answer goes nowhere.
        };
        active.phase = Phase::Ready;
        let mut props: HashMap<&'static str, Variant<'static>> = HashMap::new();
        let (reply, exchange) = match &answer {
            Answer::Password { secret, choice_chosen } => {
                active.props.choice_chosen = *choice_chosen;
                if active.props.password_new {
                    props.insert(
                        "password-strength",
                        Variant::Variant(Box::new(Variant::I32(password_strength(secret)))),
                    );
                }
                ("yes", active.exchange.send(Some(secret.as_bytes())))
            }
            Answer::Confirm { choice_chosen } => {
                active.props.choice_chosen = *choice_chosen;
                ("yes", active.exchange.send(None))
            }
            Answer::Cancel => ("no", active.exchange.send(None)),
        };
        if active.props.choice_label.is_some() {
            props.insert("choice-chosen", Variant::Variant(Box::new(Variant::Bool(active.props.choice_chosen))));
        }
        (active.key.clone(), reply, props, exchange)
    };
    drop(answer); // Zeroizes the secret.
    send_ready(&state, generation, key, reply, props, exchange);
}

/// Calls the caller's `PromptReady`. If the call fails, the caller is gone or
/// broken: end its session without `PromptDone`, as gcr does.
fn send_ready(
    state: &Shared,
    generation: u64,
    key: Key,
    reply: &'static str,
    props: HashMap<&'static str, Variant<'static>>,
    exchange: String,
) {
    let conn = state.borrow().conn.clone();
    let weak = Rc::downgrade(state);
    glib::MainContext::ref_thread_default().spawn_local(async move {
        let result = conn
            .call_method(
                Some(key.0.as_str()),
                key.1.as_str(),
                Some(CALLBACK_INTERFACE),
                "PromptReady",
                &(reply, props, exchange),
            )
            .await;
        if let Err(e) = result {
            log(&format!("PromptReady to {}: {e}", key.0));
            let Some(state) = weak.upgrade() else { return };
            if state.borrow().active.as_ref().is_some_and(|a| a.generation == generation) {
                end_session(&state, &key);
                next_ready(&state);
                update_idle(&state);
            }
        }
    });
}

fn log(message: &str) {
    eprintln!("krytis-prompter: {message}");
}
