# mise

## Tool declarations

Every repository tool is an exact version in `mise.toml`. Do not use `latest`:
it makes builds drift and Renovate cannot propose a concrete update. System C
libraries are not mise tools; `docs/skills/ci.md` documents them.

## The Rust pin

krytis compiles this crate with the Rust supplied by its freedesktop-sdk
junction. Keep the mise Rust and `package.rust-version` no newer than that compiler.
The source of truth for the current krytis pin is `origin/main`, not a local
checkout. Renovate intentionally does not update Rust here because a newer local
compiler can create a lockfile or syntax the image cannot build.

## mise.lock

Commit `mise.lock`. Refresh it only with `mise run mise-lock`; CI uses
`mise run mise-lock --check`, which regenerates and compares against Git rather
than trusting the working copy. The exact mise version affects lockfile content.

## Running several tasks

`mise run lint test` passes `test` as an argument to `lint`; it does not run two
tasks. Use `mise run lint ::: test`. Tasks that accept no arguments reject extras
so this typo cannot silently skip verification.

## Creating a task

Put executable file tasks under `mise/tasks/` with a `#MISE description` line,
`set -euo pipefail`, and explicit argument behavior. If flags need a usage spec,
add `#USAGE` lines; `mise run lint` validates every such spec. Keep packaging and
manual checks behind tasks as well: `mise run install` and `mise run live-check`.
