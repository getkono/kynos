# Releasing

Release-plz does the release. This records the parts it cannot: the one-time
setup, which lives outside the repository, and the first publish of a crate,
which crates.io does not allow a machine to perform.

This is a runbook. Nothing here is normative for implementation work.

## The loop

1. **Commits land on `master`.** `release-plz-pr` recomputes the release pull
   request from the Conventional Commits since the last tag: the version bump in
   `[workspace.package]`, and a changelog entry per crate.
2. **That pull request runs CI.** It runs unattended because the workflow
   authenticates as a GitHub App. Opened with `secrets.GITHUB_TOKEN` instead,
   its runs would sit in an approval-required state waiting for someone to
   click "Approve workflows to run".
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

## One-time setup

Everything in this section happens outside the repository. Do it in order.

### 1. Create the GitHub App — before the workflow reaches `master`

Release pull requests must be able to start their own workflow runs. Opened with
`secrets.GITHUB_TOKEN`, their runs are created in an approval-required state
that a human has to click through — which for the one pull request whose merge
publishes to crates.io is exactly the check most likely to be waved past.

Do this step *first*: with the secrets absent, the token step fails and both
release-plz jobs do nothing on every push to `master`.

1. Create a GitHub App. Set a name and a homepage URL, uncheck **Active** under
   Webhook, and grant **Repository permissions**: `Contents` read & write,
   `Pull requests` read & write. Restrict installation to this account.
2. Generate a private key and install the App on `getkono/kynos`.
3. Add the App's **Client ID** as the repository secret
   `RELEASE_PLZ_APP_CLIENT_ID` and the private key as
   `RELEASE_PLZ_APP_PRIVATE_KEY`. The Client ID, not the App ID: the action
   deprecated the `app-id` input.

A fine-grained personal access token works in the same place, at the cost of
attributing the release commit to a person and needing manual rotation.

### 2. Create the `release` label

`release-plz.toml` labels the release pull request `release`. Create it by hand
so it gets a colour and a description rather than the arbitrary one the API
assigns to a label created on first use.

### 3. Delete the abandoned release branches

Eleven `release-plz-*` branches survive from runs whose pull requests were closed
by hand. Release-plz will not reuse them.

```bash
git ls-remote --heads origin 'release-plz-*' \
  | awk '{ print $2 }' \
  | xargs -r -n1 git push origin --delete
```

### 4. Publish 0.1.0

crates.io does not accept a *new* crate through Trusted Publishing, so the first
version of each of the three needs a registry token. This is a crates.io
limitation, not a release-plz one.

That publish happens from a workstation, not from CI. `release-plz.yml` reads no
registry token and is not going to: release-plz skips the OIDC exchange whenever
one is present in the environment, so a secret wired in for this one occasion —
or left empty, or forgotten afterwards — could silently disable trusted
publishing for every release after it.

1. **Merge the release pull request first**, so the changelogs are on `master`
   and ship inside the tarballs. The `Release` job it triggers fails at
   `cargo publish` with an authentication error. That is expected and harmless:
   release-plz publishes before it tags, so the failed run leaves no tag behind
   and nothing to undo.
2. From a clean checkout of that merge commit, with a crates.io API token scoped
   to `publish-new` and `publish-update`:

   ```bash
   CARGO_REGISTRY_TOKEN=… cargo publish --workspace --all-features
   ```

   `--workspace` handles the ordering and the intra-workspace dependencies, for
   the same reason `mise run publish:check` uses it. It is not atomic: if the
   server rejects one crate, the ones already accepted stay published.
3. Revoke the token.
4. Push the tags by hand. Release-plz filters out already-published packages
   before tagging, so it will never create these three:

   ```bash
   git tag kynos-openapi-v0.1.0
   git tag kynos-macros-v0.1.0
   git tag kynos-v0.1.0
   git push origin kynos-openapi-v0.1.0 kynos-macros-v0.1.0 kynos-v0.1.0
   ```

   The tags are not cosmetic. Each changelog is generated from the commits since
   the previous tag, so without them the 0.1.1 notes would replay the entire
   history.

5. Cut the release page, which release-plz also skipped:

   ```bash
   gh release create kynos-v0.1.0 --title 'Kynos v0.1.0' \
     --notes-file <(sed -n '/^## \[0.1.0\]/,/^## \[/p' crates/kynos/CHANGELOG.md)
   ```

### 5. Configure Trusted Publishing

For each of `kynos`, `kynos-macros` and `kynos-openapi`, on crates.io:
**Settings → Trusted Publishing → Add**, platform GitHub, then

| Field | Value |
| --- | --- |
| Repository owner | `getkono` |
| Repository name | `kynos` |
| Workflow filename | `release-plz.yml` |
| Environment | *(leave empty)* |

If this is misconfigured, release-plz logs `Failed to use trusted publishing: …
Proceeding without it.` and the run then fails at `cargo publish` with an
authentication error. Check the `Release` job's log for that warning before
suspecting anything else.

### 6. Say that something is published

Two statements become false the moment 0.1.0 lands:

- [`.github/SECURITY.md`](../.github/SECURITY.md) — "Nothing is published to
  crates.io yet" becomes the supported-versions policy the same section already
  describes.
- [`README.md`](../README.md) — the status line at the top.

From 0.1.1 onward, none of this section applies: commits land, the release pull
request updates, and merging it publishes.
