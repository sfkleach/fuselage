# Definition of Shippable

This document defines what must be true before a release tag is pushed.
It complements the [Definition of Done](definition-of-done.md),
which governs individual tasks. Shippable is the release-level gate.

## Version bump

- [ ] `Cargo.toml` version is bumped appropriately (semver):
  - Patch (`x.y.Z`) — bug fixes only, no new features, no breaking changes.
  - Minor (`x.Y.0`) — new features, backwards compatible.
  - Major (`X.0.0`) — breaking changes.
- [ ] Run `cargo build` (or `cargo test`) so that `Cargo.lock` is regenerated
  to match the new version.
- [ ] Commit both `Cargo.toml` and `Cargo.lock` together.
- [ ] `Cargo.toml` version matches the intended tag (e.g. `v0.2.1`).

## CHANGELOG

- [ ] The top section of `CHANGELOG.md` is a version heading (not `Unreleased`).
- [ ] The version in the heading matches `Cargo.toml`.
- [ ] The version has not been used in any previous section (no duplicates).
- [ ] The content accurately describes all changes since the previous release.

Verify with:

```bash
just shippable
```

## Automated checks

Run `just test` and confirm it passes cleanly:

```bash
just test
```

This covers unit tests, functional tests, formatting, clippy, build, and
security audit. No failures, no warnings promoted to errors.

## Documentation

- [ ] README accurately reflects the current behaviour and options.
- [ ] Any new features are documented (options table, examples, or linked doc).
- [ ] No broken links in the README (check relative links resolve correctly).

## Release workflow

- [ ] Run the `release-check` workflow manually on GitHub and confirm it passes:
  **Actions → Release Check → Run workflow**.
- [ ] Push a draft tag (e.g. `vX.Y.Z-draft.1`) and confirm the release workflow
  builds successfully and produces the expected assets.
- [ ] Confirm the draft release on GitHub looks correct: title, release notes,
  binaries, checksum files.

## crates.io (first publish of a new crate name only)

- [ ] Confirm the crate name is not already taken:
  `curl -sSf https://crates.io/api/v1/crates/fuselage` → should 404.
- [ ] Run `cargo publish --dry-run --allow-dirty` locally and confirm it passes.

## Tagging and publishing

Confirm the working tree is clean (`git status`) — `just draft-release` will
refuse to tag a dirty tree.

```bash
just draft-release vX.Y.Z   # sign and push tag, then monitor CI manually
just publish-release vX.Y.Z # cargo publish (stable only) + flip to published
```
