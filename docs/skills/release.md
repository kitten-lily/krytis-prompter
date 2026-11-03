# Releases

release-plz maintains a release PR from Conventional Commit subjects. Merging
that PR creates `v<version>` and a GitHub Release; the crate is not published and
there are no binary assets. krytis tracks the tag and builds from source.

## What opens a release PR

`release-plz.toml` includes user-visible `feat` and `fix` commits plus explicit
lockfile maintenance. CI, docs and task-only commits do not create a release.
Renovate dependency fixes use `fix(deps)` so they appear in the dependency group.
The first matching changelog parser wins.

## The changelog

The changelog records what a release said at that time. `mise run docs-links`
skips it; later path renames must not rewrite release history. Task scopes listed
in the skip parser must include any new tooling-only task name.

## The token

A PR or tag made by the default `GITHUB_TOKEN` does not start other workflows.
Both release jobs mint a narrowly scoped GitHub App token: the release job writes
contents, and the release-PR job writes contents and pull requests.

On a new repository, leave `RELEASE_APP_CLIENT_ID` unset. The workflow jobs skip
until the app is installed and both that variable and `RELEASE_APP_PRIVATE_KEY`
exist; otherwise every push to `main` would fail at token creation.

The `v*` ruleset must permit that installed app to bypass tag creation. Do not
leave an enforcing tag ruleset with no valid creator: that blocks the first
release as well as automation.

## Previewing a release

Fetch tags, then run `mise run release-preview`. It computes the next version and
changelog without writing. The shared repository's tags are the version source;
a stale local tag set gives a false preview.

## Cutting a release

1. Merge the implementation only after Checks and `mise run live-check` pass.
2. Confirm the release workflow updated its release PR and that Checks ran on it.
3. Replace the release PR body's `<sha>` with the exact live-checked head.
4. The maintainer approves and merges the release PR.
5. Confirm the `v*` tag and GitHub Release point at that merge.

The release is a human gate because this binary enters the keyring unlock path.
