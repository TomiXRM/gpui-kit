# Release branches

`main` develops the next release. `stable` maintains the current published
series. A PR merged into `main` is automatically considered for cherry-picking
to `stable`, unless it has the exact **Breaking Changes** label.

Set the label before merging an incompatible change. Reviewing a PR still
includes deciding whether its dependencies and behavior fit the stable series;
the automation follows the label rather than inferring compatibility from code.
The repository uses squash merges, so each merged PR supplies one source commit.
Direct commits and merge commits require manual backporting.

## Automatic backports

The **Stable Branch** workflow runs after pushes to `main`, successful **Release
Crate** runs, on an hourly reconciliation schedule, or through `workflow_dispatch`.
It scans outstanding main commits in order, excluding **Breaking Changes** and
source commits already recorded in stable's history. This scan also recovers
work when GitHub concurrency replaces a pending event.

Eligible changes are cherry-picked with `-x` onto
`automation/stable-backports`. One generated PR targets `stable`, listing the
source PRs and any conflicts. A conflicting PR is aborted without preventing an
independent change from being picked; conflicts appear as workflow warnings and
in the run summary, and are retried on the next run. Resolve these by manually
backporting with `git cherry-pick -x`, or adapting the change and retaining the
`(cherry picked from commit FULL_SHA)` trailer in its commit message.

The workflow calls the existing CI workflow on the exact backport SHA, and the
documentation workflow when its path filters apply. These calls use a read-only
token. They do not depend on normal PR-event CI, which may require approval for
PRs created with `GITHUB_TOKEN`.

The **Stable backport** commit status reports the result. The generated PR is
squash-merged only after those workflows succeed, the stable base and PR head
remain unchanged, and the source PRs still lack **Breaking Changes**. Every
source SHA is retained in the squash message, so the same changes are not
backported again. Failed checks leave the PR open for inspection. Branch
protection or additional required reviews are respected by GitHub's merge API.

Adding **Breaking Changes** while the batch is being checked prevents its merge;
the next reconciliation rebuilds the batch without that change. Adding the label
after a successful backport does not revert stable. No extra inclusion label is
required.

## Publishing

Publish current-series patches from `stable` with the existing version bump and
tag workflow. Publish a new series from `main`. Only a stable `vX.Y.Z` tag whose
**Release Crate** run succeeded can promote stable; prerelease tags, failed
publishes, and older versions do not move it.

Reconciliation looks at the latest 100 successful Release Crate runs and selects
the highest stable version whose tag and crate version agree. A newer release
moves stable to that release commit. If stable already contains the released
commit, newer backports are preserved. Re-running the same release does not
reset stable. No automatic version bump or tag creation is performed.

Moving to a new series can replace stable's divergent history. Stable is a
managed branch, not a long-term branch for unreleased, stable-only features;
publish or move such work before promoting a new series. If an older series
needs support after promotion, maintain it in a separate release branch.

## Repository setup

- Create `stable` before enabling the workflow. Initially it may be cut from
  current main when those changes are compatible with the current series.
- In Actions settings, enable **Allow GitHub Actions to create and approve pull
  requests**. The implementation needs PR creation, not review approval. No
  personal token or GitHub App secret is required.
- Workflow permissions are scoped per job. Automation needs contents, pull
  requests and commit statuses write access; release reconciliation reads Actions
  runs. CI jobs receive contents read access.
- If stable has branch rules, allow the release workflow's managed promotion and
  require the **Stable backport** status for automated backports. Rules requiring
  additional approval leave the verified PR for a maintainer to merge.
- Future-version APIs such as PR #3359 belong on main with **Breaking Changes**;
  they do not need a `next` branch.

The scripts and tests use Bun:

```sh
bun test ./script/tests/stable-backport.test.ts
```

Run production reconciliation only through the workflow. It changes local
checkout state and writes branches, PRs and statuses; it is not a read-only
preview command.
