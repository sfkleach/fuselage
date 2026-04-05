# fuselage

[![Build and Test](https://github.com/sfkleach/fuselage/actions/workflows/build-and-test.yml/badge.svg?branch=main)](https://github.com/sfkleach/fuselage/actions/workflows/build-and-test.yml)

Run a command with ephemeral, namespace-private filesystems derived from zip archives.
No containers, no daemons, no manual cleanup.

## Overview

`fuselage` creates a private Linux mount namespace for a command, unpacks zip archives into
it, then execs the command. When the command exits the mounts vanish automatically.

There is no isolation beyond the mount namespace — the process keeps its normal environment,
PID space, and network.

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

### setuid-root mode

`fuselage` is designed to run setuid-root. It needs `CAP_SYS_ADMIN` to create a
mount namespace, and drops privileges back to the real user before execing the
child — so the child process runs as the invoking user with no UID remapping.

```bash
sudo chown root:root /usr/local/bin/fuselage
sudo chmod u+s /usr/local/bin/fuselage
```

### Unprivileged mode

Not all users will be comfortable with the setuid-root requirement. For that
reason, `fuselage` has a fall-back strategy that does not require setuid-root.
In this case it falls back to a user namespace (`unshare --user --mount
--map-root-user`), which works on most Linux distributions but will remap the
process to uid 0 inside the namespace.

This allows you to use `fuselage` in a wide variety of scenarios where the
apparent user-id does not matter. However, tools like `sudo` will behave
unexpectedly in this mode.

## Installation

Install with the one-liner (installs setuid-root to `$HOME/.local/bin` by default):

```bash
curl -sSfL https://raw.githubusercontent.com/sfkleach/fuselage/main/install.sh | bash
```

Or without setuid (UID remapping fallback):

```bash
curl -sSfL https://raw.githubusercontent.com/sfkleach/fuselage/main/install.sh | FUSELAGE_SETUID=0 bash
```

Or download a pre-built binary directly from the [releases page](https://github.com/sfkleach/fuselage/releases)
and install setuid-root as shown below.

```bash
# Download to (say) ~/.local/bin/fuselage for your architecture. This example
# assumes 64-bit Intel and moves the binary to ~/.local/bin, which is typically
# on your $PATH. IMPORTANT: replace the version number with the current release.
wget https://github.com/sfkleach/fuselage/releases/download/v0.1.0/fuselage-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
tar zxf fuselage-v0.1.0-x86_64-unknown-linux-gnu.tar.gz fuselage
mv -i fuselage ~/.local/bin
rm -f fuselage-v0.1.0-x86_64-unknown-linux-gnu.tar.gz

# setuid-root for normal setup (optional).
sudo chown root:root ~/.local/bin/fuselage
sudo chmod u+s ~/.local/bin/fuselage
```

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

This project is licensed under the [MIT License](LICENSE).
