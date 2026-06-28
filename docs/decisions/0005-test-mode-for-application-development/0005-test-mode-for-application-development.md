# 0005 - Test mode for application development: --test=FOLDER, 2026-06-28

## Issue

**Leap.** Applications that run inside a fuselage environment depend on
`FUSELAGE_DYNAMIC`, `FUSELAGE_STATIC`, and `FUSELAGE_TMPDIR` being set and
populated. During development this is a nuisance: either the developer runs
the full fuselage invocation (requiring real archives, namespace privileges, and
extraction overhead) or they manually mock the environment variables. Neither is
ergonomic for a tight edit-run-debug loop.

The decision is to add a `--test=FOLDER` option that lets fuselage wire up the
environment from pre-existing fixture directories rather than from archives,
skipping all namespace and extraction work.

## Factors

- Developers need a fast inner loop that does not require building archives or
  holding namespace privileges.
- The fixture directories that serve as test data may also be the canonical
  source material from which real archives are eventually produced (e.g. via
  `mksquashfs` or `zip`). The design should not work against this dual role.
- Test runs can leave state in scratch directories. Retaining that state is
  valuable for debugging; accidentally inheriting leftover state from a previous
  run is a common source of false results.
- The production command line (including `--extract=allow`, `--extract=prefer`,
  etc.) should work unchanged when `--test` is added, so that the developer
  tests the real invocation rather than a stripped-down approximation.

## Decision

Add `--test=FOLDER` which puts fuselage into test mode:

- All namespace creation, archive extraction, and mount operations are skipped.
- `FUSELAGE_DYNAMIC`, `FUSELAGE_STATIC`, and `FUSELAGE_TMPDIR` are set to
  subdirectories of `FOLDER` matching the layout fuselage would have created.
- `--extract=*` flags are silently ignored — they are irrelevant in test mode
  but must be accepted so the same command line works in both modes.
- Archive file paths in `--dynamic` and `--static` arguments are also silently
  ignored; only the `NAME` portion is used to locate the fixture directory.

### Fixture layout

```
FOLDER/
  dynamic/
    NAME/     ← --dynamic=NAME:ignored.zip  (fixture; must exist)
  static/
    NAME/     ← --static=NAME:ignored.sfs   (fixture; must exist)
  tmp/        ← FUSELAGE_TMPDIR             (scratch)
  dynamic/
    NAME/     ← --dynamic-empty=NAME        (scratch; created empty)
```

Fixture directories (`--dynamic` and `--static`) must exist before the run;
fuselage hard-fails if they are absent. The archive file paths are never
validated or opened.

Scratch directories (`tmp/` and `--dynamic-empty` targets) are managed by
fuselage: created if absent, checked for emptiness on entry, and cleaned on
exit by default.

### Scratch handling and `--clean-and-retain`

The default behaviour for scratch directories is strict:

- **Entry**: hard-fail if any scratch directory is non-empty. Leftover content
  from a previous run indicates a test setup bug rather than expected state.
- **Exit**: remove the contents of scratch directories (the directories
  themselves are left in place).

With `--clean-and-retain`:

- **Entry**: wipe scratch directories clean regardless of their current
  contents. No failure.
- **Exit**: leave scratch directories intact. The developer can inspect what
  the application wrote during the run.

## Consequences

### What this buys

- Developers can work against fixture directories without archives, privileges,
  or extraction overhead.
- The same command line (with `--test=FOLDER` added) tests the real invocation,
  including any `--extract` policy.
- Fixture directories serve double duty as the source material for eventual
  archive production, so there is no throwaway scaffolding to discard later.
- `--clean-and-retain` gives a straightforward debugging path for inspecting
  application output without requiring extra tooling.

### What this gives up

- `--test` mode provides no namespace isolation and no read-only enforcement —
  it is explicitly a development convenience, not a production substitute.
- Silently ignoring `--extract` and archive paths means fuselage does not warn
  when flags that would have effect in production are present but inert. This
  is an accepted trade-off for command-line symmetry.

## Additional Notes

The fixture directory layout mirrors the procdir layout fuselage creates in
normal operation, making it straightforward to convert a working test fixture
into a real archive:

```bash
# Develop against fixtures
fuselage --test=fixtures --static=sdk:sdk.sfs -- myapp

# When ready, package the fixture as the real archive
mksquashfs fixtures/static/sdk sdk.sfs

# Production invocation — identical except --test is removed
fuselage --static=sdk:sdk.sfs -- myapp
```
