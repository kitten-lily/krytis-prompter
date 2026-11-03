# AGENTS.md

krytis-prompter is the standalone GTK4 provider for
`org.gnome.keyring.SystemPrompter` on krytis. It handles passwords in the user
session unlock path. Correct protocol ordering and a small secret boundary matter
more than convenience.

Read `README.md`, this file and `docs/SKILL.md` before changing anything. Load only
the skill files relevant to the task.

## Work and learning

A change has two outputs: the implementation and any non-obvious lesson learned
while producing it. Put reusable protocol, UI, testing or release lessons in the
matching `docs/skills/` file in the same commit as the change. Do not append task
status, obvious Rust facts or temporary failures. Correct stale nearby guidance
instead of only adding prose.

## Branches and pull requests

- Fetch `origin`, then create a feature branch and worktree from `origin/main`.
  Never push directly to `main`.
- Commits use Conventional Commits, are signed, and include an AI attribution
  trailer when an AI assistant contributed.
- Merge commits only. Never squash or rebase a pull request on GitHub.
- The maintainer approves and merges. No WIP pull requests.

## Tasks and verification

- Every repeatable maintenance action is a `mise` task. Do not add an unwrapped
  repository script.
- `mise run lint ::: test` is the minimum pull-request gate. Run
  `mise run docs-links` for documentation changes.
- Runtime changes also require `mise run live-check`: observe the real GTK prompt,
  enter the requested phrase, and confirm the daemon idle-exits afterward.
- Installation changes require a staged `DESTDIR=<dir> mise run install` smoke and
  inspection of all three installed files.
- A D-Bus or secret-handling change requires the real libgcr client interop test,
  not only a unit test or mock.

## Human gates

Stop for maintainer approval before merging any of these:

- **Security:** secret handling, libgcr FFI, D-Bus caller isolation or name ownership.
- **Breakage:** unlock-path behavior, activation or installation paths.
- **Design:** new protocol behavior, dependencies, UI behavior or subsystem shape.
- **Release:** creating a `v*` tag or GitHub Release.

Implementation and testing may proceed after the maintainer chooses the design.
The maintainer remains accountable for every decision.

## Invariants

- No cryptography in this repository. `GcrSecretExchange` from libgcr-4 owns
  `sx-aes-1`.
- Reply to `BeginPrompting` before calling `PromptReady`. Keep the raw
  `zbus::MessageStream`; an interface macro does not guarantee this ordering.
- One active prompting session. Queue others in order, bind every callback to its
  unique bus sender, and remove queued and active work when that sender vanishes.
- Never log a secret or place it unencrypted on D-Bus. Clear GTK entry buffers and
  zeroize the Rust-owned copy after handing it to libgcr.
- Claim the bus name with `DO_NOT_QUEUE`; if another prompter owns it, exit cleanly.
- The idle timer runs only with no active or queued session.
- `Cargo.lock` is committed. krytis vendors exactly that dependency graph.
