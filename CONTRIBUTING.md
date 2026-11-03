# Contributing

Issues and pull requests are welcome. For a security problem, follow
[`SECURITY.md`](SECURITY.md) instead of opening an issue.

## Setup

Every tool comes from [mise](https://mise.jdx.dev), pinned in `mise.toml`, and every
maintenance step is a mise task (`mise tasks` lists them). In a fresh clone, run
`mise trust` once. Building also needs the GTK 4, gtk4-layer-shell and gcr-4
development files, and the tests need `dbus-daemon`; [`README.md`](README.md)
§ Development lists the tasks.

## Before you open a pull request

- `mise run lint ::: test` passes. The `:::` matters: `mise run lint test` runs only
  `lint`.
- If you changed docs, `mise run docs-links` passes.
- If you changed what the prompter does at runtime, run the live check
  ([`README.md`](README.md) § Testing), and say in the PR what you saw.
- If you changed how the typed secret is handled, or the D-Bus surface, say so at the
  top of the PR: those changes wait for the maintainer's review before they merge
  ([`AGENTS.md`](AGENTS.md) § Tasks and verification).
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org).
  Releases are cut from them ([`docs/skills/release.md`](docs/skills/release.md)):
  `feat` and `fix` are for changes to the program, `ci` and `chore` for tooling.
- Sign your commits, so GitHub shows them as Verified.

Pull requests are merged with a merge commit, never squashed or rebased.

## AI-assisted contributions

AI tools are allowed. This repository itself is largely written with AI coding
agents, and [`AGENTS.md`](AGENTS.md) is their entry point. Two rules:

1. **Disclose it.** Say in the pull request description which tool you used and what
   it did, or add an `Assisted-by: <tool>` trailer to the commits it helped write.
   The same applies to issues and security reports.
2. **You own the result.** Read and understand every line before you submit it, and
   run the checks above yourself. Don't submit generated code, reports or review
   replies you haven't verified; they will be closed.

## Where things are documented

[`AGENTS.md`](AGENTS.md) holds the rules every change follows, for people and agents
alike, and [`docs/SKILL.md`](docs/SKILL.md) routes to the notes for each area.
