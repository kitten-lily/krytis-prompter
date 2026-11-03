# krytis-prompter

A standalone GTK4 provider for `org.gnome.keyring.SystemPrompter`, built for
[krytis](https://github.com/starlit-os/krytis). It gives gnome-keyring, oo7 and
other GCR clients a password/confirmation dialog in niri or another Wayland
session without depending on a desktop shell.

## Design

- Rust with zbus 6 for the GCR prompter protocol.
- GTK4 plus gtk4-layer-shell: overlay layer, exclusive keyboard focus, normal
  toplevel fallback.
- libgcr-4 `GcrSecretExchange` for `sx-aes-1`. This repository contains no crypto
  implementation; plaintext secrets never cross D-Bus.
- One active session with a FIFO waiting queue, caller-disconnect cleanup, and a
  60-second idle exit that pauses while a session exists.
- noctalia-compatible GTK colors, font and corner radius, with a dark fallback.

Protocol details and the reply-ordering invariant are in
[`docs/skills/protocol.md`](docs/skills/protocol.md).

## Installation

The runtime needs GTK 4, gtk4-layer-shell and gcr-4. A staged installation:

```sh
DESTDIR="$PWD/stage" mise run install
```

This installs:

| Path | Purpose |
|---|---|
| `/usr/bin/krytis-prompter` | daemon |
| `/usr/share/dbus-1/services/org.gnome.keyring.SystemPrompter.service` | session-bus activation |
| `/usr/lib/systemd/user/krytis-prompter.service` | `systemd --user` service |

The session bus activates the user unit on the first `BeginPrompting`. If another
prompter already owns the name, this daemon exits without queueing behind it.

```text
usage: krytis-prompter [--idle-timeout SECONDS]
```

`SECONDS` defaults to 60; zero disables idle exit. `--version` prints the release.

## Development

Install the GTK 4, gtk4-layer-shell and gcr-4 development files plus
`pkg-config` and `dbus-daemon`, then install the pinned tools:

```sh
mise trust
mise install
mise run lint ::: test
mise run docs-links
```

`Cargo.lock` is committed because krytis's BuildStream element vendors this exact
dependency graph.

## Testing

`mise run test` runs unit tests and a headless integration test with a private
`dbus-daemon`, the real libgcr `GcrSystemPrompt` client, and the real
`GcrSecretExchange` round trip. It covers warning re-prompts, new passwords,
confirm, choices, Cancel, queueing, caller disconnect and idle exit.

For an implementation or UI change, also run:

```sh
mise run live-check
```

It opens the actual GTK layer-shell prompt on a private bus. Enter the exact phrase
shown; the check verifies the decrypted bytes and waits for idle exit. Visually
confirm that the dialog is centered, above fullscreen content, keyboard-exclusive,
masked, and themed correctly.

## Security

This program handles unlock secrets in the user session. Report vulnerabilities
privately as described in [`SECURITY.md`](SECURITY.md). Changes to secret handling,
D-Bus isolation, activation or the unlock path require maintainer review before
merge.

Licensed under the [MIT License](LICENSE).
