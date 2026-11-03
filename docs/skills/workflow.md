# Workflow

## Branches and worktrees

Fetch `origin`, then create a named branch and sibling worktree from
`origin/main`. Never build a feature on a stale local `main`, and never pull,
merge or reset another person's checkout without permission.

## Commits

Use `<type>(<scope>): <description>`, sign every commit, and add an
`Assisted-by:` trailer when AI helped. A non-obvious lesson belongs in the same
commit as the code that revealed it. `Cargo.lock` changes stay with the dependency
or source change that produced them.

## Merging

The repository allows merge commits only. The maintainer uses GitHub's merge
button after required Checks, review, and the applicable security/breakage/design
gates. Do not squash or rebase: preserving signed topic commits is intentional.

Renovate's `automergeStrategy` is therefore `merge-commit`. Release PRs remain
human-merged because their merge creates a release.

## Pull-request evidence

State only checks actually run. Runtime changes report the exact head tested with
`mise run live-check`; install changes report the staged paths. Do not duplicate
GitHub's CI status in comments. One review comment per event; edit it rather than
posting a correction as a second comment.
