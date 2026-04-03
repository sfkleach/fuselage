# fuselage

Run a command with ephemeral, namespace-private filesystems derived from zip archives.
No containers, no daemons, no manual cleanup.

## Overview

`fuselage` creates a private Linux mount namespace for a command, unpacks zip archives into
it, then execs the command. When the command exits the mounts vanish automatically.

There is no isolation beyond the mount namespace — the process keeps its normal environment,
PID space, network, and UID.

## Usage

```
fuselage [OPTIONS...] [--] COMMAND [ARG...]
fuselage [OPTIONS...] --run PATH [ARG...]
```

### Options

| Option | Description |
|---|---|
| `--dynamic=[NAME:]FILE` | Extract `FILE` into a fresh, mutable directory at `$FUSELAGE_DYNAMIC/NAME/` |
| `--static=[NAME:]FILE` | Extract `FILE` into a cached, read-only directory at `$FUSELAGE_STATIC/NAME/` |
| `--run PATH [ARG...]` | Find `PATH` in extracted archives and execute it |

`FILE` is a zip file or a text file containing base64-encoded zip data.
`NAME` defaults to the filename stem (e.g. `my-data.zip` → `my-data`).
All archive names must be unique; use `NAME:` to disambiguate.

### Environment variables set for the child process

| Variable | Set when | Value |
|---|---|---|
| `FUSELAGE_TMPDIR` | Always | Ephemeral scratch directory |
| `FUSELAGE_DYNAMIC` | Any `--dynamic` | Parent of all dynamic extractions |
| `FUSELAGE_STATIC` | Any `--static` | Parent of all static extractions |

## Examples

```bash
# Ephemeral scratch space
fuselage bash
echo "$FUSELAGE_TMPDIR"   # writable, gone when bash exits

# Cached SDK + throwaway working copy
fuselage --static=sdk:toolchain.zip --dynamic=src:source.zip \
    -- make -C "$FUSELAGE_DYNAMIC/src" -j4

# Run an executable from inside an archive
fuselage --dynamic=app:my-app.zip --run bin/server --port 8080
```

## Privilege model

`fuselage` needs `CAP_SYS_ADMIN` to call `mount(2)`. By default it uses a user namespace
(`unshare --user --mount --map-root-user`) so no installation or elevated privileges are
required. The process sees itself as uid 0 inside the namespace, but files on real
filesystems are owned by the real user.

Running as root skips the user namespace entirely.

## Installation

```bash
cargo install --path .
```

Or download a pre-built binary from the [releases page](../../releases).

## Building from source

Requires Rust stable (edition 2024, Rust ≥ 1.85) and a Linux kernel with user namespace
support (most distributions enable this by default).

```bash
cargo build --release
# binary at target/release/fuselage
```

Run the test suite:

```bash
cargo test
```

Or via [just](https://just.systems):

```bash
just test
```

## License

MIT
