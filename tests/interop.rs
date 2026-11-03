mod common;

use std::{collections::HashMap, sync::mpsc, thread, time::Duration};

use common::{Bus, GcrPrompt, Script, UiEvent, start_daemon, start_daemon_held, use_bus_for_gcr};
use futures_util::StreamExt;
use gtk::glib;
use krytis_prompter::{
    daemon::{self, INTERFACE, PATH},
    exchange::Exchange,
    ui::PromptKind,
};
use zbus::{OwnedObjectPath, OwnedVariant};

/// Runs every protocol check in one test because GIO caches the process's
/// session-bus connection after the first GcrSystemPrompt opens it.
#[test]
fn real_gcr_client_and_lifecycle() {
    let bus = Bus::start();
    use_bus_for_gcr(&bus);
    let daemon = start_daemon(&bus, None);

    let prompt = GcrPrompt::open(10).expect("gcr client opens a prompting session");
    prompt.title("Keyring unlock");
    prompt.message("Enter the login password");
    daemon.ui.answer(Script::Password("fóx battery 9!"));
    assert_eq!(prompt.password().unwrap().unwrap(), "fóx battery 9!".as_bytes());

    prompt.warning("The unlock password was incorrect");
    daemon.ui.answer(Script::Password("second try"));
    assert_eq!(prompt.password().unwrap().unwrap(), b"second try");
    let events = daemon.ui.events();
    let UiEvent::Prompt(second) = &events[1] else { panic!("second UI event was not a prompt: {events:?}") };
    assert_eq!(second.props.warning.as_deref(), Some("The unlock password was incorrect"));

    prompt.choice_label("Remember this choice");
    daemon.ui.answer(Script::PasswordChoice("with choice", true));
    assert_eq!(prompt.password().unwrap().unwrap(), b"with choice");
    assert!(prompt.choice_chosen());

    prompt.password_new(true);
    daemon.ui.answer(Script::Password("long New Password 7!"));
    assert_eq!(prompt.password().unwrap().unwrap(), b"long New Password 7!");
    assert!(prompt.password_strength() > 0);
    let events = daemon.ui.events();
    let UiEvent::Prompt(new_password) = &events[3] else { panic!("fourth UI event was not a prompt: {events:?}") };
    assert!(new_password.props.password_new);

    daemon.ui.answer(Script::Confirm { choice_chosen: false });
    assert!(prompt.confirm().unwrap());
    daemon.ui.answer(Script::Cancel);
    assert_eq!(prompt.password().unwrap(), None, "Cancel is PromptReady(\"no\")");
    prompt.close().unwrap();
    assert!(matches!(daemon.ui.wait_events(7).last(), Some(UiEvent::Close)));

    queue_waits_for_the_active_session(&daemon.ui);
    queued_stop_sends_prompt_done(&bus);
    caller_disconnect_closes_the_active_prompt(&bus, &daemon.ui);
    // A separate bus proves the daemon's idle timer exits it. The active daemon
    // above has no timeout and remains available to the GIO connection cached
    // by the real gcr client.
    let idle_bus = Bus::start();
    let idle = start_daemon(&idle_bus, Some(Duration::from_millis(75)));
    assert_eq!(idle.done.recv_timeout(Duration::from_secs(2)).unwrap(), Ok(()));

    activating_call_is_served();
}

/// D-Bus activation starts the daemon because of a call, and the bus delivers
/// that call as soon as the name is claimed: before the daemon would read
/// anything if it subscribed to its connection only after claiming. The caller
/// then got no reply, the idle timer exited the daemon, and gcr's client gave
/// up with "Operation was cancelled" (krytis's artifact gate, 2026-11-03).
///
/// Here the call reaches the daemon while it is held between connecting and
/// claiming the name. "Reaches" is proven, not waited for: dbus-daemon handles
/// one connection's messages in order, so once it answers the client's GetId it
/// has already routed the BeginPrompting sent before it. Only then is the
/// daemon released, so a slow runner can't let the test pass vacuously.
fn activating_call_is_served() {
    let bus = Bus::start();
    let held = start_daemon_held(&bus);
    let address = bus.address.clone();
    let destination = held.unique_name.clone();
    let (routed_tx, routed_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let client = thread::spawn(move || {
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            ctx.block_on(async move {
                let (ready, mut ready_rx) = futures_channel::mpsc::unbounded();
                let (done, _done_rx) = futures_channel::mpsc::unbounded();
                let conn = zbus::connection::Builder::address(address.as_str()).build().await.unwrap();
                let callback = OwnedObjectPath::try_from("/test/Activated").unwrap();
                conn.object_server().at(callback.clone(), Callback { ready, done }).await.unwrap();

                // Sent by hand, not through a proxy, so the reply wait can start
                // after the barrier below.
                let mut replies = zbus::MessageStream::from(&conn);
                let begin = zbus::Message::method_call(PATH, "BeginPrompting")
                    .destination(destination.as_str())
                    .interface(INTERFACE)
                    .build(&(&callback,))
                    .unwrap();
                let serial = begin.primary_header().serial_num();
                conn.send(&begin).await.unwrap();
                zbus::fdo::DBusProxy::new(&conn).await.unwrap().get_id().await.unwrap();
                routed_tx.send(()).unwrap();

                let begun = loop {
                    let Some(Ok(msg)) = replies.next().await else { break Err("stream ended".to_string()) };
                    if msg.header().reply_serial() != Some(serial) {
                        continue;
                    }
                    break match msg.message_type() {
                        zbus::message::Type::MethodReturn => Ok(()),
                        other => Err(format!("{other:?}: {msg:?}")),
                    };
                };
                let ready = begun.is_ok() && ready_rx.next().await.is_some();
                result_tx.send((begun, ready)).unwrap();
            });
        })
        .unwrap();
    });

    routed_rx.recv_timeout(Duration::from_secs(5)).expect("the client never routed its BeginPrompting");
    held.go.send(()).unwrap();
    let (begun, ready) =
        result_rx.recv_timeout(Duration::from_secs(5)).expect("the activating BeginPrompting got no reply");
    assert_eq!(begun, Ok(()), "BeginPrompting failed");
    assert!(ready, "no PromptReady after the activating BeginPrompting");
    client.join().unwrap();
}

fn queue_waits_for_the_active_session(ui: &common::Ui) {
    ui.clear();
    let first = GcrPrompt::open(10).unwrap();
    let (opened_tx, opened_rx) = mpsc::channel();
    let (close_tx, close_rx) = mpsc::channel();
    let second = thread::spawn(move || {
        let result = GcrPrompt::open(10);
        opened_tx.send(result.as_ref().map(|_| ()).map_err(Clone::clone)).unwrap();
        let prompt = result.unwrap();
        close_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        prompt.close().unwrap();
    });

    assert!(
        opened_rx.recv_timeout(Duration::from_millis(250)).is_err(),
        "the queued session opened before the active one closed"
    );
    first.close().unwrap();
    opened_rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    close_tx.send(()).unwrap();
    second.join().unwrap();
}

fn queued_stop_sends_prompt_done(bus: &Bus) {
    let active = GcrPrompt::open(10).unwrap();
    let address = bus.address.clone();
    let client = thread::spawn(move || {
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            ctx.block_on(async move {
                let (ready, _ready_rx) = futures_channel::mpsc::unbounded();
                let (done, mut done_rx) = futures_channel::mpsc::unbounded();
                let conn = zbus::connection::Builder::address(address.as_str()).build().await.unwrap();
                let callback = OwnedObjectPath::try_from("/test/QueuedPrompt").unwrap();
                conn.object_server().at(callback.clone(), Callback { ready, done }).await.unwrap();
                let proxy = zbus::Proxy::new(&conn, daemon::BUS_NAME, PATH, INTERFACE).await.unwrap();

                proxy.call_method("BeginPrompting", &callback).await.unwrap();
                proxy.call_method("StopPrompting", &callback).await.unwrap();
                done_rx.next().await.expect("queued StopPrompting sends PromptDone");
            });
        })
        .unwrap();
    });

    client.join().unwrap();
    active.close().unwrap();
}

struct Callback {
    ready: futures_channel::mpsc::UnboundedSender<()>,
    done: futures_channel::mpsc::UnboundedSender<()>,
}

#[zbus::interface(name = "org.gnome.keyring.internal.Prompter.Callback")]
impl Callback {
    async fn prompt_ready(&self, _reply: &str, _properties: HashMap<String, OwnedVariant>, _exchange: &str) {
        let _ = self.ready.unbounded_send(());
    }

    async fn prompt_done(&self) {
        let _ = self.done.unbounded_send(());
    }
}

fn caller_disconnect_closes_the_active_prompt(bus: &Bus, ui: &common::Ui) {
    ui.clear();
    ui.answer(Script::Hold);
    let address = bus.address.clone();
    let (shown_tx, shown_rx) = mpsc::channel();
    let (disconnect_tx, disconnect_rx) = mpsc::channel();
    let client = thread::spawn(move || {
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            ctx.block_on(async move {
                let (ready, mut ready_rx) = futures_channel::mpsc::unbounded();
                let (done, _done_rx) = futures_channel::mpsc::unbounded();
                let conn = zbus::connection::Builder::address(address.as_str()).build().await.unwrap();
                let callback = OwnedObjectPath::try_from("/test/Prompt").unwrap();
                conn.object_server().at(callback.clone(), Callback { ready, done }).await.unwrap();
                let proxy = zbus::Proxy::new(&conn, daemon::BUS_NAME, PATH, INTERFACE).await.unwrap();
                proxy.call_method("BeginPrompting", &callback).await.unwrap();
                ready_rx.next().await.expect("initial PromptReady");

                let exchange = Exchange::new();
                let payload = exchange.begin();
                let properties: HashMap<String, OwnedVariant> = HashMap::new();
                proxy.call_method("PerformPrompt", &(callback, "password", properties, payload)).await.unwrap();
                shown_tx.send(()).unwrap();
                disconnect_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                drop(proxy);
                drop(conn);
            });
        })
        .unwrap();
    });

    shown_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let shown = ui.wait_events(1);
    let UiEvent::Prompt(request) = &shown[0] else { panic!("caller did not show a prompt: {shown:?}") };
    assert_eq!(request.kind, PromptKind::Password);
    disconnect_tx.send(()).unwrap();
    client.join().unwrap();
    let events = ui.wait_events(2);
    assert!(matches!(events.as_slice(), [UiEvent::Prompt(_), UiEvent::Close]));

    let next = GcrPrompt::open(10).expect("another caller opens after the active caller disconnects");
    next.close().unwrap();
}
