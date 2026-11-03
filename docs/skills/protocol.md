# Prompter protocol

## Surface

The daemon owns `org.gnome.keyring.SystemPrompter` and serves
`org.gnome.keyring.internal.Prompter` at `/org/gnome/keyring/Prompter`.
A session is identified by the calling connection's unique bus name plus its
callback object path.

1. `BeginPrompting(callback)` queues the session.
2. When it becomes active, the daemon calls `PromptReady("", {}, exchange)`.
3. `PerformPrompt(callback, type, properties, exchange)` updates persistent
   properties and opens either a password or confirm prompt.
4. The answer is `PromptReady("yes"|"no", changed_properties, exchange)`.
5. `StopPrompting(callback)` closes an active UI, sends `PromptDone()`, and
   advances the queue. A caller may stop while it is still queued; it still
   receives `PromptDone()` even though it never received `PromptReady()`.

Only one session is active. A `NameOwnerChanged` signal with no new owner removes
all work belonging to that unique name. No `PromptDone` is sent to a vanished
caller.

## Reply ordering

The reply to `BeginPrompting` must reach gcr before the initial `PromptReady`.
`GcrSystemPrompt` arms its callback wait only after the method reply; reversed
ordering ends the session as “Another prompt is already in progress.” Read calls
from a raw `zbus::MessageStream`, await `Connection::reply`, then schedule the
callback. Do not replace this with `#[zbus::interface]` or `ObjectServer` without a
real-client test proving the ordering.

The same rule applies to `PerformPrompt`: send its empty method reply before
showing a UI that can eventually trigger `PromptReady`.

## Activation

Subscribe before claiming the name. When the session bus activates the daemon,
the call that triggered it is already queued, and the bus delivers it the moment
`RequestName` succeeds. zbus drops a message that arrives while no
`MessageStream` exists, so a daemon that claims first and subscribes afterwards
never sees that `BeginPrompting`: the caller gets no reply, the idle timer exits
the daemon, and gcr's client gives up with "Operation was cancelled".
`daemon::Prompter::new` subscribes (method calls and `NameOwnerChanged`) when it
is built, and `claim_name` and `run` hang off it, so the order can't be reversed.

v0.1.0 had this bug: it claimed in `main` and subscribed in `run`. No test here
saw it, because the interop test and `live-check` start the daemon before the
client calls; only real activation reverses that. krytis's
`mise run krytis-prompter-test` found it on 2026-11-03, driving the built
artifact through its own activation file. `activating_call_is_served` in
`tests/interop.rs` now holds the daemon between connecting and claiming while
a `BeginPrompting` is on the wire.

## Secret exchange

`src/exchange.rs` is deliberately a four-call FFI boundary around libgcr-4:
`gcr_secret_exchange_new`, `_begin`, `_receive`, and `_send`. libgcr performs the
Diffie-Hellman exchange, key derivation and AES for `sx-aes-1`. Do not port crypto
into Rust or substitute oo7's public Secret Service exchange; oo7's public backend
uses a different group.

One `GcrSecretExchange` belongs to one prompting session and survives warning
re-prompts in that session. A new caller/session gets a new object. Receive the
caller's exchange before sending a secret. Clear GTK buffers and drop the
`Zeroizing<String>` immediately after `_send` returns.

## Returned properties are nested variants

gcr 4.4's `GcrSystemPrompt` iterates the returned `a{sv}` and then calls
`g_variant_get_variant` on each value. With zbus, put a `Variant::Variant`
around `choice-chosen` and `password-strength`; a plain bool or i32 is unboxed
once by GLib and triggers `g_variant_get_variant` criticals, so the client never
updates the property. The real-client test asserts both values. This is a wire
compatibility quirk, not an invitation to double-wrap incoming properties.

## Interop tests

`tests/interop.rs` starts a private `dbus-daemon`, the real protocol core with a
scripted UI, and libgcr-4's real `GcrSystemPrompt` client. It covers byte-for-byte
password delivery, warning re-prompt, new-password strength, choices, confirm,
Cancel, queue ordering, caller disconnect and idle exit. It needs no display.

A unit test cannot replace it: the consumer owns the callback timing and decrypts
the `sx-aes-1` payload. Run it through `mise run test` so release mode and the
committed lockfile are exercised.

The daemon tests run on a private `glib::MainContext`. Schedule local futures and
timers with `MainContext::ref_thread_default().spawn_local`; GLib's
`idle_add_local_once` and `timeout_add_local_once` target the global default
context and silently stall this harness. Keep the returned `JoinHandle` so an
idle timer can be aborted when a session arrives.
