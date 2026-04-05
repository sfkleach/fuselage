# Task: Publishing fuselage as a Rust Package

The purpose of this task is to make it possible for people to install
`fuselage` using `cargo install fuselage`.

## Background

`Cargo.toml` already has all required metadata fields (`name`, `version`,
`license`, `description`, `repository`, `readme`, `keywords`, `categories`,
`rust-version`). The release workflow already includes a `publish-crate` job
that runs `cargo publish` on stable release tags (i.e. tags without a `-`
suffix). What remains is one-time setup and a README update.

## Steps

### Step 1 — Verify the crate package locally

Run the following before the first publish attempt:

```bash
cargo package --list
```

Check that the tarball contains the expected files and nothing sensitive
(no `.env`, credentials, large binaries, etc.). Also do a dry-run publish:

```bash
cargo publish --dry-run
```

This exercises the full publish path (compilation, metadata validation) without
actually uploading anything.

### Step 2 — Create a crates.io API token

1. Log in at [crates.io](https://crates.io) (GitHub OAuth).
2. Go to **Account Settings → API Tokens → New Token**.
3. Give it a descriptive name (e.g. `fuselage-github-actions`).
4. Select scopes: `publish-new` and `publish-update`.
5. Copy the token — it is shown only once.

### Step 3 — Add the token as a GitHub Actions secret

In the repository on GitHub:
**Settings → Secrets and variables → Actions → New repository secret**

- Name: `CARGO_REGISTRY_TOKEN`
- Value: the token from Step 2.

The `publish-crate` job in `.github/workflows/release.yml` already reads this
secret.

### Step 4 — Update README installation instructions

Add a `cargo install` one-liner to the Installation section of `README.md`,
below the `curl` one-liner:

```bash
cargo install fuselage
```

Note: `cargo install` does not set the setuid bit. After installing, users
should follow the setuid-root instructions in the privilege model section to
get full functionality.

### Step 5 — Release pre-check

Check we are release ready by:

- [ ] Review the definition-of-done.
- [ ] Manually running the release-check.yml workflow.
- [ ] Update the README.md installation so that it references the new version (e.g. v0.2.0).
- [ ] Publish a draft (e.g. v0.2.0-draft.1) and confirm it builds.

### Step 6 — Trigger a release

Push a stable version tag (no `-` suffix, e.g. `v0.2.0`). The release
workflow will build binaries, create the GitHub release, and publish to
crates.io automatically.

The first publish claims the `fuselage` name on crates.io — verify it is
not already taken before tagging.

## Notes

- Crate versions are immutable on crates.io. A published version can be yanked
  (which discourages use) but not deleted or modified.
- Pre-release tags (`-rc`) and draft tags (other `-` suffixes) are excluded
  from publishing by the `if: ${{ !contains(github.ref_name, '-') }}` condition
  in the workflow.
