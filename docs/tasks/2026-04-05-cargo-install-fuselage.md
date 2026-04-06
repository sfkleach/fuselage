# Task: Publishing fuselage as a Rust Package

The purpose of this task is to make it possible for people to install
`fuselage` using `cargo install fuselage`.

## Background

`Cargo.toml` already has all required metadata fields (`name`, `version`,
`license`, `description`, `repository`, `readme`, `keywords`, `categories`,
`rust-version`). `cargo publish` runs locally from the developer's workstation
as part of `just publish-release` — it does NOT run from GitHub Actions. This
is a deliberate security decision: crates.io is a separate trust domain from
GitHub, and keeping `cargo publish` local ensures a compromised GitHub account
cannot push a malicious crate. See
[docs/decisions/0002-designing-installsh](../decisions/0002-designing-installsh/0002-designing-installsh.md)
for the full analysis.

## Steps

### Step 1 — Verify the crate package locally [DONE]

```bash
cargo package --list
cargo publish --dry-run
```

Confirmed: 12 files, compiles cleanly, dry run passes.

### Step 2 — Create a crates.io API token [DONE]

Token created at crates.io with `publish-new` and `publish-update` scopes.
Stored in the local cargo config (`~/.cargo/credentials.toml`) — NOT in
GitHub Actions secrets.

### Step 3 — Update README installation instructions [DONE]

`cargo install fuselage` added to the Installation section of `README.md`
with a note that `cargo install` does not set the setuid bit.

### Step 4 — Release pre-check

Before each release, verify shippable state per
[docs/process/definition-of-shippable.md](../process/definition-of-shippable.md):

- [ ] `just test` passes cleanly.
- [ ] `just shippable` passes (CHANGELOG version matches Cargo.toml, no duplicates).
- [ ] Manually run the `release-check.yml` workflow on GitHub and confirm it passes.
- [ ] Push a draft tag (e.g. `v0.2.0-draft.1`) and confirm the release workflow
  builds and produces the expected assets.

### Step 5 — Trigger a release

```bash
just draft-release vX.Y.Z    # sign tag, push, wait for CI to complete
just publish-release vX.Y.Z  # cargo publish locally + flip GitHub draft to published
```

The first publish claims the `fuselage` name on crates.io — verify it is
not already taken before the first tag.

## Notes

- Crate versions are immutable on crates.io. A published version can be yanked
  (which discourages use) but not deleted or modified.
- Pre-release (`-rc`) and draft (`-`) tags are not published to crates.io —
  `just publish-release` skips the `cargo publish` step for those tags.
- `CARGO_REGISTRY_TOKEN` must NOT be added as a GitHub Actions secret. The
  token lives only in `~/.cargo/credentials.toml` on the developer's machine.
