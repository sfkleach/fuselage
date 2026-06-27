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
| `--dynamic-empty=NAME` | Create an empty writable directory at `NAME` (no archive needed) |
| `--static=[NAME:]FILE` | Mount or extract `FILE` into a read-only directory at `$FUSELAGE_STATIC/NAME/` |
| `--cache-static` | Convert zip `--static` archives to squashfs for faster subsequent runs (requires `mksquashfs`) |
| `--run PATH` | Find `PATH` in extracted archives and execute it |
| `--extract=POLICY` | `deny` (default) / `allow` / `force` — controls use of namespace-free extract-and-run mode |

`FILE` is a zip file, a squashfs image (`.sfs`), an ELF binary with an embedded squashfs image, or a
text file containing base64-encoded zip or squashfs data.
`NAME` defaults to the filename stem (e.g. `my-data.zip` → `my-data`, `my-data.sfs` → `my-data`).
All archive names must be unique; use `NAME:` to disambiguate.

`NAME` may also be an absolute path of the form `/run/fuselage/NAME` (one component under that prefix),
causing the archive to be mounted at that fixed location. This allows pre-built environments with
hardcoded paths (e.g. Python venvs) to work correctly. Fixed-path mounts require setuid-root mode.

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

# Build a Python venv at a fixed path, then capture it as a squashfs
fuselage --dynamic-empty=/run/fuselage/myapp -- bash -c '
  uv venv /run/fuselage/myapp/.venv && uv pip install -r requirements.txt
  mksquashfs /run/fuselage/myapp myapp.sfs
'

# Run the captured venv (loop-mounted at the same fixed path)
fuselage --static=/run/fuselage/myapp:myapp.sfs -- \
  /run/fuselage/myapp/.venv/bin/python -m myapp
```

## fuselage-bundle

`fuselage-bundle` packages a squashfs archive and a baked-in fuselage invocation
into a single self-executing ELF binary that can be distributed and run directly.

```bash
fuselage-bundle --archive=myapp.sfs --output=myapp \
  -- --static=/run/fuselage/myapp:/proc/self/exe \
     --run /run/fuselage/myapp/.venv/bin/python \
     -- -m myapp
```

The resulting `myapp` binary locates `fuselage` on `PATH`, mounts its own embedded
squashfs at `/run/fuselage/myapp`, and runs the Python module. Pass `--keep` to
retain the intermediate build directory for inspection.

## uv-bundle

`uv-bundle` packs a [uv](https://docs.astral.sh/uv/)-managed Python project into a
single self-executing ELF binary, running the whole pipeline for you: it creates a
private tmpfs with `fuselage --dynamic-empty`, installs the project's dependencies
into it with `uv sync`, compresses it with `mksquashfs`, and packs the result with
`fuselage-bundle`.

```bash
uv-bundle --project=./myapp --output=myapp
```

By default the bundle runs `python -m <package>`, where the package name is read
from `pyproject.toml` (override the module with `--module=MOD`). To run a
[`[project.scripts]`](https://packaging.python.org/en/latest/specifications/entry-points/)
console script directly instead, use `--script=NAME`; it runs `.venv/bin/NAME`
rather than `python -m`, and is mutually exclusive with `--module`:

```bash
uv-bundle --project=./myapp --script=greet --output=greet
```

Other options: `--dev` (include dev dependencies), `--squashfs=PATH` (keep the
intermediate squashfs), `--uv-arg=ARG` (forward an argument to `uv sync`,
repeatable), and `--verbose` (print the squashfs tree and sizes).

`uv-bundle` always builds against the **system** Python (it sets
`UV_PYTHON_PREFERENCE=system`), not a uv-managed download, so the bundled venv
references a stable interpreter path rather than one under the build user's home
directory. Before building, the system Python is checked against
`[project].requires-python`; a mismatch is an error unless
`--ignore-version-mismatch` is given.

Requires `fuselage` (setuid), `fuselage-bundle`, `uv`, `mksquashfs`, and `gcc`.

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

### Extract-and-run mode (`--extract=allow` / `--extract=force`)

In environments where user namespaces are disabled (e.g. some containers and
hardened hosts), `fuselage` can run without creating any namespace at all.
This is the same escape-hatch strategy used by AppImage's
`--appimage-extract-and-run` flag for running in containers where FUSE is
unavailable.
Pass `--extract=force` to always use this mode, or `--extract=allow` to fall
back to it automatically when `unshare` fails (a warning is printed when the
fallback is taken).

**Known limitations in extract-and-run mode:**

- **`--static` archives are not read-only.** In normal namespace mode the kernel
  enforces read-only on static mounts (`EROFS`). In extract-and-run mode there
  is no mount namespace and therefore no bind-remount-ro. File permissions
  cannot substitute: the child process owns the extracted files and can restore
  write permission at will. See [docs/decisions/0004](docs/decisions/0004-extract-mode-no-readonly-enforcement/0004-extract-mode-no-readonly-enforcement.md)
  for the full analysis.
- **Fixed-path mounts (`/run/fuselage/NAME`) are unavailable** and produce a
  hard error if requested.

The extractions remain ephemeral regardless: the procdir is cleaned up when the
command exits.

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
wget https://github.com/sfkleach/fuselage/releases/download/v0.4.0/fuselage-v0.4.0-x86_64-unknown-linux-gnu.tar.gz
tar zxf fuselage-v0.4.0-x86_64-unknown-linux-gnu.tar.gz fuselage fuselage-bundle
mv -i fuselage fuselage-bundle ~/.local/bin
rm -f fuselage-v0.4.0-x86_64-unknown-linux-gnu.tar.gz

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
