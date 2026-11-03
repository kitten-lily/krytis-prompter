# Renovate

Renovate updates Cargo dependencies, mise tools, GitHub Actions, annotated
mise-action input versions, and the base image digest of
`containers/live-check.Containerfile`. It opens PRs against `main`; patch/minor
dependency updates may auto-merge only after required checks pass. Rust itself is
disabled because it follows krytis's freedesktop-sdk compiler. The Containerfile's
Ubuntu takes digest updates only: it mirrors CI's `ubuntu-26.04` runner, and moves
to a new release together with `.github/workflows/checks.yml`.

## Hosted app provisioning

Renovate runs as the hosted Mend Renovate app; there is no workflow in this
repository. Land `.github/renovate.json5` on `main` before enabling the repository
in Renovate's dashboard. If the app sees a default branch with no config, it opens
an onboarding PR instead of applying this policy. After enabling it, the Dependency
Dashboard issue is the proof that the hosted app indexed the repository.

## Auto-merge needs at least one check

Renovate's platform auto-merge waits for branch protection and check results. A
repository with no required/visible check can remain pending indefinitely or merge
without the intended gate. Keep the `Checks` workflow on pull requests and the
main ruleset requiring its `Checks` job.

## Vulnerability alerts

`osvVulnerabilityAlerts` covers direct and transitive Cargo lockfile entries.
GitHub vulnerability alerts remain enabled independently. An alert is a triage
input: verify that the vulnerable code reaches this binary before deciding on an
exception, and document any exception with an expiry or upstream fix reference.

## Silent drops under pinDigests

Renovate's mise manager can parse a workflow `version:` input and then drop it
under digest pinning because it is not a `uses:` digest. The regex manager's
`renovate:` comments anchor the mise version in both workflows so the values move
together. Keep the annotation beside each input.

## Checking the config

Run `mise run renovate-check` for syntax and resolved policy, and
`mise run renovate-check --dry-run` when credentials and network access are
available. The task uses Renovate's container; do not replace it with an
unversioned `npx renovate`.
