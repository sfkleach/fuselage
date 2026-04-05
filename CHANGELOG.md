# Change Log for Fuselage

Following the style in https://keepachangelog.com/en/1.0.0/

## v0.1.0, Base64 Encoded Archive 2026-04-05

### Added

- Install pre-built binaries via `curl ... | bash` script. See [README.md](./README.md).
- Published to [crates.io](https://crates.io/crates/fuselage): `cargo install fuselage`.
- Base64 archive format: `--static` and `--dynamic` now accept files containing
  a base64-encoded zip or squashfs image, with optional leading `#` comment lines.
  The decoded archive is written into the private tmpfs and never touches persistent
  storage. This enables self-contained executable scripts that embed their own
  filesystem payload — see [docs/combining-with-herescript.md](docs/combining-with-herescript.md).

## v0.1.0, Initial release 2026-04-04

### Added

- Basic `fuselage COMMAND [ARG...]` usage: enters a private mount namespace,
  sets `FUSELAGE_TMPDIR` to an ephemeral tmpfs scratch directory, forks/execs
  the command, waits for it, and cleans up on exit.
- Setuid-root mode: when the binary is installed setuid-root, privileges are
  dropped to the real user before exec so that `sudo` works correctly inside
  the namespace.
- `--dynamic` / `-d [NAME:]FILE` option: extracts a zip or squashfs archive
  into a fresh, writable directory under `$FUSELAGE_DYNAMIC/NAME/`, private
  to the invocation and discarded on exit.
- `--static` / `-s [NAME:]FILE` option: extracts a zip or squashfs archive
  into a read-only directory under `$FUSELAGE_STATIC/NAME/`. The directory
  is bind-mounted read-only inside the namespace.
- Squashfs support: `.sfs` files are accepted as input for both `--static`
  and `--dynamic`. In privileged mode, static squashfs images are loop-mounted
  directly; in user-namespace mode they are extracted via the `backhand` library.
- `--cache-static` flag: opt-in persistent caching of zip archives as squashfs
  images under `~/.fuselage/cache/`, keyed by SHA-256 content hash. Disabled
  by default so confidential archives leave no trace on disk.
- Cache expiry: on exit, a double-forked background reaper evicts cache entries
  older than `FUSELAGE_CACHE_MAX_AGE_DAYS` (default 30 days). A 60-second
  recency guard prevents races with concurrent fuselage processes. Orphaned
  partial build artefacts (no `.complete` sentinel, older than 1 hour) are
  also removed.
- `--run PATH [ARG...]` option: resolves a relative path whose first component
  names a mounted archive, then execs the found executable with any remaining
  arguments.
- Archive name validation: derived and explicit names are checked to be
  non-empty, free of path separators and null bytes, and not `.` or `..`.
  Files whose stem would be empty (e.g. `.zip`) are rejected with a clear
  error message.
- Unit tests for pure and I/O-testable functions: `stem()`, `validate_name()`,
  `detect_format()`, `compute_sha256()`, `reap_cache()`, and
  `parse_archive_specs()`.
- Functional test suite (`tests/functest.sh`) covering plain (user-namespace)
  and setuid modes, exercising all major options and error paths.
- Decision records for the `--cache-static` opt-in design and the cache
  expiry mechanism.
