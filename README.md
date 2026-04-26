# fuselage

[![Build and Test](https://github.com/sfkleach/fuselage/actions/workflows/build-and-test.yml/badge.svg?branch=main)](https://github.com/sfkleach/fuselage/actions/workflows/build-and-test.yml)

Run a command with ephemeral, namespace-private filesystems derived from zip or squashfs archives.
No containers, no daemons, no manual cleanup.

## Overview

`fuselage` creates a private Linux mount namespace for a command, unpacks zip or squashfs
archives into it, then execs the command. When the command exits the mounts vanish automatically.

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
| `--static=[NAME:]FILE` | Mount or extract `FILE` into a cached, read-only directory at `$FUSELAGE_STATIC/NAME/` |
| `--cache-static` | Convert zip `--static` archives to squashfs for faster subsequent runs (requires `mksquashfs`) |
| `--run PATH [ARG...]` | Find `PATH` in extracted archives and execute it |

`FILE` is a zip file, a squashfs image (`.sfs`), or a text file containing base64-encoded zip or squashfs data.
`NAME` defaults to the filename stem (e.g. `my-data.zip` → `my-data`, `my-data.sfs` → `my-data`).
All archive names must be unique; use `NAME:` to disambiguate.

Squashfs images are the most efficient format for `--static` archives: in setuid-root mode they are
loop-mounted read-only directly from the file, with no extraction to disk. Create them with `mksquashfs`
from the `squashfs-tools` package.

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

Several installation methods are available. None requires `root` to install,
but setting the setuid bit (for the recommended operating mode) always does.

### `cargo install` (builds from source)

Requires the Rust toolchain — install via [rustup.rs](https://rustup.rs) if needed.

```bash
cargo install fuselage
# And make it setuid-root (optional).
sudo chown root:root ~/.cargo/bin/fuselage
sudo chmod u+s ~/.cargo/bin/fuselage
```

### `curl | bash`

A convenience one-liner for casual use (installs to `$HOME/.local/bin` setuid-root
by default). See [install.sh security notice](install.sh) before using.

```bash
curl -sSfL https://raw.githubusercontent.com/sfkleach/fuselage/main/install.sh | bash
```

Or without setuid (UID remapping fallback):

```bash
curl -sSfL https://raw.githubusercontent.com/sfkleach/fuselage/main/install.sh | FUSELAGE_SETUID=0 bash
```

### Pre-built binary via `cargo binstall`

Requires [cargo-binstall](https://github.com/cargo-bins/cargo-binstall). Downloads
a pre-built binary from the GitHub release and verifies its SHA256 checksum before
installing. Falls back to compiling from source if no pre-built binary is available
for your target.

```bash
cargo binstall fuselage
# And make it setuid-root (optional).
sudo chown root:root ~/.cargo/bin/fuselage
sudo chmod u+s ~/.cargo/bin/fuselage
```

### Manual download

Download a pre-built binary directly from the [releases page](https://github.com/sfkleach/fuselage/releases)
and install setuid-root as shown below.

```bash
# Download to (say) ~/.local/bin/fuselage for your architecture. This example
# assumes 64-bit Intel and moves the binary to ~/.local/bin, which is typically
# on your $PATH. IMPORTANT: replace the version number with the current release.
wget https://github.com/sfkleach/fuselage/releases/download/v0.2.0/fuselage-v0.2.0-x86_64-unknown-linux-gnu.tar.gz
tar zxf fuselage-v0.2.0-x86_64-unknown-linux-gnu.tar.gz fuselage
mv -i fuselage ~/.local/bin
rm -f fuselage-v0.2.0-x86_64-unknown-linux-gnu.tar.gz

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

## Further reading

- [Combining fuselage with herescript](docs/combining-with-herescript.md) — embed a filesystem payload directly inside an executable script

## License

This project is licensed under the [MIT License](LICENSE).
