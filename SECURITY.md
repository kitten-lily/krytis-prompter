# Security policy

## Reporting a vulnerability

Report it privately through GitHub:
[Security → Report a vulnerability](https://github.com/kitten-lily/krytis-prompter/security/advisories/new).
Please don't open a public issue for it, and don't put a working exploit in a pull
request.

Include what you saw, the release (`krytis-prompter --version`) or commit, the
caller that started the prompt (gnome-keyring, oo7, `secret-tool`, …), and how to
reproduce it. This is a small project maintained in spare time: expect a reply within
two weeks, and a fix in a new release if the report holds up. A fix lands in krytis
with the next image that builds that release.

## Supported versions

Only the latest release gets fixes. Releases are the `v*` tags.

## What is in scope

krytis-prompter sits in the keyring unlock path. It runs as your user, outside any
sandbox, started by the session bus on the first call to
`org.gnome.keyring.SystemPrompter`. It:

- **owns the name `org.gnome.keyring.SystemPrompter`** on the session bus and serves
  the GCR prompter protocol (`org.gnome.keyring.internal.Prompter` at
  `/org/gnome/keyring/Prompter`) to whichever process calls it, calling back that
  caller's `org.gnome.keyring.internal.Prompter.Callback` object;
- **shows a GTK 4 dialog** on a layer-shell overlay and reads the password, or the
  confirmation, that you type into it;
- **returns the typed secret encrypted with `sx-aes-1`**, through libgcr-4's
  `GcrSecretExchange` (Diffie-Hellman key agreement, then AES), so the plaintext never
  crosses the bus;
- **reads the noctalia theme** from your configuration to colour the dialog.

In scope:

- **The typed secret**: anything that lets it reach a process other than the caller
  that asked for it, reach the bus unencrypted, outlive the prompt in this process's
  memory longer than needed, or be written to a log, a file or the screen.
- **The `sx-aes-1` exchange**: a flaw in how this crate drives `GcrSecretExchange`
  (key reuse across prompts or callers, a secret sent before the caller's key is
  received, a downgrade to another protocol). Bugs in gcr's own crypto belong to
  [GNOME's gcr](https://gitlab.gnome.org/GNOME/gcr); tell us too, since we would carry
  the fix.
- **The D-Bus surface**: a caller, or any other process on the session bus, that can
  make the prompter answer a different session's prompt, read another session's reply,
  wedge the prompter for everyone else, or crash it with a crafted message or property.
- **Name ownership**: anything that lets another process take
  `org.gnome.keyring.SystemPrompter` away from krytis-prompter, or own it first and
  harvest the passwords it is shown, beyond what any process of your user can already
  do on your session bus. Say which of those it is.
- **A misleading dialog**: caller-supplied text that can make the prompt look like
  it comes from somewhere it doesn't.
- A dependency advisory that reaches code this program runs: the Rust crates in
  `Cargo.lock`, and gtk4, gtk4-layer-shell and gcr-4 as krytis ships them.

Out of scope: a process that already runs as your user and attacks you by other
means (ptrace, reading your keyring files, injecting input into the compositor);
gnome-keyring's and oo7's own handling of the secret once they have decrypted it.
