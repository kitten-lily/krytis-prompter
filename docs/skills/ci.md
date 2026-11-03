# Continuous integration

## System libraries

Rust tools come from mise, but C libraries belong to the runner image. Checks on
Ubuntu 26.04 install `libgtk-4-dev`, `libgtk4-layer-shell-dev`, `libgcr-4-dev`,
`pkgconf`, and `dbus-daemon`. The first four satisfy `pkg-config` probes from the
`-sys` crates; the last runs the private-bus tests. Keep crate feature floors at
or below Ubuntu's GTK 4.22.2 unless the runner changes.

The package names and versions were checked against Ubuntu resolute on
2026-11-03. Recheck the workflow rather than treating this dated inventory as
current forever.

## The Rust toolchain

`jdx/mise-action` installs the exact Rust from `mise.toml`. A runner also has a
system Rust, so `mise run toolchain-check` compares `rustc --version` with the
configured pin; a successful `cargo --locked` alone does not prove the intended
toolchain ran.

## Pinning mise

The action version and its `version:` input are both pinned. The action SHA is a
supply-chain boundary; the exact mise version makes `mise.lock` deterministic.
Renovate updates the annotated version in checks and release workflows together.

## Writing the workflow

Use full action SHAs with version comments and least-privilege job permissions.
The multi-task command must be quoted: `run: "mise run lint ::: test"`; unquoted
`:::` followed by a space can be parsed as YAML syntax instead of a scalar.
