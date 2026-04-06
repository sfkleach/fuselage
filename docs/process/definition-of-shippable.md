# Definition of Shippable

This document defines what must be true before a release tag is pushed.
It complements the [Definition of Done](../working-practices/definition-of-done.md),
which governs individual tasks. Shippable is the release-level gate.

## Automated checks

Run `just test` and confirm it passes cleanly:

```bash
just test
```

This covers unit tests, functional tests, formatting, clippy, build, and
security audit. No failures, no warnings promoted to errors.

## CHANGELOG

- [ ] The top section of `CHANGELOG.md` is a version heading (not `Unreleased`).
- [ ] The version in the heading matches `Cargo.toml`.
- [ ] The version has not been used in any previous section (no duplicates).
- [ ] The content accurately describes all changes since the previous release.

Verify with:

```bash
just shippable
```

## Version numbers

- [ ] `Cargo.toml` version is bumped appropriately (semver):
  - Patch (`x.y.Z`) — bug fixes only, no new features, no breaking changes.
  - Minor (`x.Y.0`) — new features, backwards compatible.
  - Major (`X.0.0`) — breaking changes.
- [ ] `Cargo.toml` version matches the CHANGELOG heading.
- [ ] `Cargo.toml` version matches the intended tag (e.g. `v0.2.0`).

## Documentation

- [ ] README accurately reflects the current behaviour and options.
- [ ] Any new features are documented (options table, examples, or linked doc).
- [ ] No broken links in the README (check relative links resolve correctly).

## Release workflow

- [ ] Run the `release-check` workflow manually on GitHub and confirm it passes:
  **Actions → Release Check → Run workflow**.
- [ ] Push a draft tag (e.g. `v0.2.0-draft.1`) and confirm the release workflow
  builds successfully and produces the expected assets.
- [ ] Confirm the draft release on GitHub looks correct: title, release notes,
  binaries, checksum files.

## crates.io (first publish of a new crate name only)

- [ ] Confirm the crate name is not already taken:
  `curl -sSf https://crates.io/api/v1/crates/fuselage` → should 404.
- [ ] Run `cargo publish --dry-run --allow-dirty` locally and confirm it passes.

## Tagging and publishing

Once all checks above pass:

```bash
just draft-release vX.Y.Z   # push tag, wait for CI, mirror checksums
just publish-release vX.Y.Z # verify checksums present, flip to published
```

See the [cargo-install task](../tasks/2026-04-05-cargo-install-fuselage.md)
for full details of the two-phase release process.
