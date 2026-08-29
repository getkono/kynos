# Releasing

Release-plz does the release. This records the parts it cannot: the loop a human
drives, and the Trusted Publishing config that lives on crates.io rather than in
this repository.

This is a runbook. Nothing here is normative for implementation work.

## The loop

1. **Commits land on `master`.** `release-plz-pr` recomputes the release pull
   request from the Conventional Commits since the last tag: the version bump in
   `[workspace.package]`, and a changelog entry per crate.
2. **Ask that pull request for its CI.** It arrives with no checks. GitHub does
   not let a pull request opened with `secrets.GITHUB_TOKEN` start workflow runs
   unattended, so a human has to either approve the held runs or close and
   reopen the pull request, which re-attributes the event to a person. Do this
   before reading the changelog, not after: this is the one merge here that
   publishes to crates.io, and it is the one whose CI does not start itself.
3. **Merging it publishes.** `release-plz-release` publishes `kynos-openapi`,
   then `kynos-macros`, then `kynos` — an order release-plz derives from the
   dependency graph, waiting for each to appear in the index before starting the
   next — then tags all three and cuts one GitHub release, `Kynos vX.Y.Z`.

`release_always = false`, so a push that is not a release-PR merge releases
nothing. Release-plz recognises the merge by the `release-plz-` branch prefix, so
a hand-made pull request meant to trigger a release has to use that prefix.

All three crates always carry the same version. They inherit
`version.workspace`, and release-plz gives every inheriting member the highest
next version it computed for any of them. `kynos` re-exports the other two, so a
`kynos` pinning an older `kynos-openapi` would re-export a document model it does
not ship.

Publishing happens before tagging. A failed publish therefore leaves no tag
behind, and the run is safe to retry once the cause is fixed.

## Before handing off

Any change touching a manifest, a feature or a shipped file:

```bash
mise run publish:check
```

It packages all three crates and rebuilds each from its own tarball, which is
what `cargo publish` will do. CI runs the same task on every push, in the
`Package` job.

## What the pipeline does not check

- **cargo-semver-checks compares default features only**, and treats any failure
  it cannot parse as "compatible". The verdict in the release pull request body
  is evidence for a reviewer, not a gate. [`nfr.md`](nfr.md) records it as
  `partial` for this reason.
- **The changelog skips `refactor`, `test`, `style`, `build`, `ci` and `chore`.**
  A breaking commit of any of those types is still listed:
  `protect_breaking_commits` overrides every skip rule, and it has to, because 32
  of the 46 breaking commits in the history so far are `refactor!:`.

## Trusted Publishing

Release-plz authenticates to crates.io with a short-lived registry token it
mints through Trusted Publishing, so no registry credential is stored in this
repository. The config lives on crates.io rather than here, one entry per crate
— `kynos`, `kynos-macros` and `kynos-openapi` — under **Settings → Trusted
Publishing → Add**, platform GitHub:

| Field | Value |
| --- | --- |
| Repository owner | `getkono` |
| Repository name | `kynos` |
| Workflow filename | `release-plz.yml` |
| Environment | *(leave empty)* |

If an entry is missing or misconfigured, release-plz logs `Failed to use
trusted publishing: … Proceeding without it.` and the run then fails at
`cargo publish` with `no token found`. Check the `Release` job's log for that
warning before suspecting anything else. Publishing precedes tagging, so such a
run leaves no tag behind and is safe to retry once the config is fixed.

A fourth crate would need this done again before its first release, and cannot
have it done in advance: crates.io does not accept a *new* crate through Trusted
Publishing, so a name that does not exist on the registry yet has to be published
once from a workstation. That is a crates.io limitation, not a release-plz one;
the three crates here were published that way for 0.1.0. Do not wire a registry
token into CI for the occasion — see below. Instead:

1. **Merge the release pull request first**, so the changelogs are on `master`
   and ship inside the tarballs, and so the already-published crates the new one
   depends on are on the index for it to resolve against. The `Release` job that
   merge triggers fails at `cargo publish`; that is expected, and publishing
   precedes tagging, so it leaves nothing to undo.
2. From a clean checkout of that merge commit, with a crates.io token scoped to
   `publish-new` and `publish-update`:

   ```bash
   CARGO_REGISTRY_TOKEN=… cargo publish --workspace --all-features
   ```

   `--workspace` handles the ordering and the intra-workspace dependencies, for
   the same reason `mise run publish:check` uses it. It is not atomic: if the
   server rejects one crate, the ones already accepted stay published.
3. Revoke the token, and add the new crate's Trusted Publishing entry.
4. Push the tags by hand — release-plz filters out already-published packages
   before tagging, so it creates none of them:

   ```bash
   git tag kynos-vX.Y.Z && git push origin kynos-vX.Y.Z   # and one per crate
   ```

   The tags are not cosmetic. Each changelog is generated from the commits since
   the previous tag, so a missing one replays the entire history into the next
   release's notes.
5. Cut the release page, which release-plz also skipped:

   ```bash
   gh release create kynos-vX.Y.Z --title 'Kynos vX.Y.Z' \
     --notes-file <(sed -n '/^## \[X.Y.Z\]/,/^## \[/p' crates/kynos/CHANGELOG.md)
   ```

`release-plz.yml` reads no `CARGO_REGISTRY_TOKEN` and must not be given one.
Release-plz skips the OIDC exchange whenever a registry token is present in the
environment, so a secret wired in for an occasion like the one above — or left
empty, or forgotten afterwards — could silently disable trusted publishing for
every release after it.

No other secret is part of this pipeline either. `release-plz.yml` authenticates
with `secrets.GITHUB_TOKEN`, which every run already has, so there is no App to
create, no key to rotate and no repository secret this pipeline reads. What that
costs is step 2 of [the loop](#the-loop), where a human asks the release pull
request for its CI. Closing *that* gap later means a GitHub App or a fine-grained
personal access token in place of `secrets.GITHUB_TOKEN` in both jobs.
Repository-wide Actions permission to create pull requests does not close it:
that setting grants the right to open the pull request, not the ability of the
pull request to trigger runs — release PR #17 sat with zero checks while the
setting was on.
