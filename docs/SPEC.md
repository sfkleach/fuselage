# fuselage — ephemeral virtual filesystems for processes

## Overview

`fuselage` runs a command with ephemeral, namespace-private filesystems derived
from zip or squashfs archives. It requires no containers, no daemons, and no
manual cleanup.

There is no isolation: the process keeps its normal environment, PID space,
network, and UID. The only thing `fuselage` adds is a private mount namespace
so that (a) the ephemeral mounts are invisible to other processes and (b) they
vanish automatically when the command exits.

See also `fuselage-bundle`, a companion tool that packages a squashfs archive
and a fuselage invocation into a single self-executing ELF binary.

## Usage

```
fuselage [OPTIONS...] [--] COMMAND [ARG...]
fuselage [OPTIONS...] --run PATH [ARG...]
```

All options are optional. With no `--static`, `--dynamic`, or `--dynamic-empty`
flags, fuselage simply provides an ephemeral tmpdir at `$FUSELAGE_TMPDIR`.

### `--dynamic=[NAME:]FILE`

Extract `FILE` into a fresh, mutable directory under `$FUSELAGE_DYNAMIC/`.
The extraction is private to this invocation and is discarded when the
command exits.

`FILE` must be one of:

- A zip file (magic bytes `PK\x03\x04`).
- A squashfs image (magic bytes `hsqs` or `sqsh`).
- An ELF binary with an embedded squashfs image (magic bytes `\x7fELF`).
- A text file containing base64-encoded zip or squashfs data (with optional
  leading `#` comment lines).

`NAME` is optional. If omitted, it is derived from the filename by stripping
the extension (e.g. `my-data.zip` → `my-data`). The archive is extracted into
`$FUSELAGE_DYNAMIC/NAME/`.

Alternatively, `NAME` may be an absolute path of the form `/run/fuselage/NAME`
(a single path component under that prefix). In this case the archive is
extracted into that fixed path rather than under `$FUSELAGE_DYNAMIC/`. Fixed
paths require setuid-root mode; see [Privilege model](#privilege-model).

### `--static=[NAME:]FILE`

Like `--dynamic`, but:

- Squashfs images are loop-mounted read-only directly from the file in
  setuid-root mode (no extraction). In unprivileged mode they are extracted
  to a directory and bind-mounted read-only.
- ELF+squashfs files are treated like squashfs, with the kernel receiving the
  byte offset of the embedded squashfs image.
- Zip archives are extracted and optionally cached; see `--cache-static`.
- The directory appears at `$FUSELAGE_STATIC/NAME/`.

As with `--dynamic`, `NAME` may be an absolute path `/run/fuselage/NAME` for
fixed-path mounting; this requires setuid-root mode.

### `--dynamic-empty=NAME`

Create an empty, writable directory at `NAME` without extracting any archive.
`NAME` may be a relative name (directory created under `$FUSELAGE_DYNAMIC/`) or
an absolute path `/run/fuselage/NAME`. This is the build-time counterpart to
`--static=/run/fuselage/NAME:FILE`: use `--dynamic-empty` to create the fixed
path during a build invocation, populate it, then capture the result as a
squashfs for distribution.

### `--cache-static`

Convert zip `--static` archives to squashfs images and cache them under
`~/.fuselage/cache/`, keyed by SHA-256 content hash. Subsequent runs with the
same file skip conversion and mount the cached squashfs directly. Disabled by
default so that confidential archives leave no traces on disk.

### `--run PATH`

Search all mounted archives (both static and dynamic) for `PATH`. If found in
exactly one archive, execute it with any arguments that follow `--`. Errors if:

- `PATH` is not found in any archive.
- `PATH` is found in more than one archive (ambiguous).
- The found file is not executable.

`--run` is an alternative to `-- COMMAND`; use one or the other. Requires at
least one `--static`, `--dynamic`, or `--dynamic-empty` archive.

### `--extract=POLICY`

Controls whether extract-and-run mode is used. In this mode no `unshare(2)` is
called and no mount namespace is created, so fuselage works in environments
where unprivileged user namespaces are disabled (e.g.
`kernel.unprivileged_userns_clone=0` or a seccomp filter blocking `unshare`).
Accepted values:

- **`deny`** (default) — never use extract-and-run; fail with a clear error if
  the required namespace cannot be created.
- **`allow`** — prefer namespace mode, but fall back to extract-and-run
  automatically if `unshare(2)` fails. A warning is emitted when the fallback
  is taken, since the namespace isolation guarantee is silently dropped.
- **`force`** — always use extract-and-run; skip namespace creation entirely.

Fixed-path mounts (`/run/fuselage/NAME`) are incompatible with extract-and-run
mode; fuselage aborts with a clear error if both are requested. See
[Extract-and-run mode](#extract-and-run-mode) in the privilege model for further
limitations.

### Archive name uniqueness

All relative archive names must be unique across `--static`, `--dynamic`, and
`--dynamic-empty` within a single invocation. If two archives would have the
same derived name, use the `NAME:` prefix to disambiguate:

```
fuselage --static=cached:foo.zip --dynamic=fresh:foo.zip -- ...
```

Fixed absolute paths (`/run/fuselage/NAME`) must also be unique within an
invocation.

### `COMMAND [ARG...]`

The command to execute. It runs as the calling user with full access to the
normal filesystem, plus the additional mounts.

## Archive formats

`--static` and `--dynamic` accept the following file types, detected by magic
bytes at the start of the file:

| Format | Detection |
|---|---|
| Zip | Magic bytes `PK\x03\x04` at offset 0 |
| Squashfs | Magic bytes `hsqs` (little-endian) or `sqsh` (big-endian) at offset 0 |
| ELF+squashfs | ELF magic `\x7fELF` at offset 0; squashfs at a computed page-aligned offset |
| Base64 | Any file whose content is valid base64 text (with optional `#` comment lines) |

For ELF+squashfs the squashfs offset is the 4096-byte-aligned end of the ELF's
complete on-disk data: the maximum of the end of the section-header table, the
program-header table, and the last PT_LOAD segment. The squashfs magic bytes are
verified at the computed offset; an ELF without an embedded squashfs is rejected.

This layout is the same as AppImage Type 2, so existing AppImage files work as
archive inputs.

## Directory layout

```
~/.fuselage/
  procdirs/<pid>/              per-process ephemeral root (tmpfs)
    tmp/                       scratch space ($FUSELAGE_TMPDIR)
    dynamic/NAME/              --dynamic extractions (relative names)
    static/NAME/               --static read-only bind-mounts (relative names)
  cache/<sha256-prefix>.sfs    --cache-static squashfs image (when mksquashfs available)
  cache/<sha256-prefix>/       --cache-static extracted directory (mksquashfs fallback)
  cache/<sha256-prefix>.complete  extraction-complete sentinel

/run/fuselage/
  NAME/                        fixed-path mounts (absolute-name archives)
```

- `procdirs/<pid>/` is overlaid with a tmpfs inside the mount namespace. Other
  processes see it as an empty directory. It is removed on exit.
- `cache/` lives on the real filesystem and persists across invocations. Each cache entry has a
  `<sha256-prefix>.complete` sentinel plus either a `<sha256-prefix>.sfs` squashfs image (when
  `mksquashfs` is available) or a `<sha256-prefix>/` extracted directory as a fallback.
- `/run/fuselage/` is created during the privilege window if it does not exist.
  Inside the private mount namespace, `/run/fuselage/NAME` is visible only to
  the fuselage invocation that created it; concurrent invocations do not
  interfere.

## Environment variables

| Variable | Set when | Value |
|---|---|---|
| `FUSELAGE_TMPDIR` | Always | `~/.fuselage/procdirs/<pid>/tmp` |
| `FUSELAGE_DYNAMIC` | Any relative-name `--dynamic` | `~/.fuselage/procdirs/<pid>/dynamic` |
| `FUSELAGE_STATIC` | Any relative-name `--static` | `~/.fuselage/procdirs/<pid>/static` |

These are inherited by child processes. Fixed-path archives (`/run/fuselage/NAME`)
do not affect `FUSELAGE_DYNAMIC` or `FUSELAGE_STATIC`.

## Privilege model

`fuselage` needs to call `mount(2)`, which requires `CAP_SYS_ADMIN`.

### Setuid-root mode

When the binary is installed setuid-root, the privilege window is minimal:

1. Save the caller's real UID/GID.
2. Create `/run/fuselage/` and any fixed-path subdirectories (while root).
3. `unshare(CLONE_NEWNS)` — create a private mount namespace.
4. Mount a tmpfs over the procdir and create per-invocation subdirectories.
5. Loop-mount or extract each archive into its destination.
6. `chown` mount roots to the real user.
7. `seteuid` / `setresuid` — permanently and irreversibly drop all privileges.
8. `exec` the command as the original user.

This preserves normal UID semantics: the child process sees its real UID, `sudo`
works inside the namespace, and file ownership is straightforward.

Fixed-path mounts (`/run/fuselage/NAME`) and loop devices require this mode.
Attempting to use them without setuid-root produces a clear error.

### Unprivileged mode

When run as a normal user without setuid, fuselage creates a user namespace
(`unshare --user --mount --map-root-user`). Inside the namespace the process
appears as uid 0, which is sufficient for `mount(2)`. Squashfs images are
extracted (not loop-mounted), and fixed-path mounts are unavailable.

### Running as root

If the caller is already root (e.g. via `sudo fuselage ...`), fuselage uses a
plain mount namespace with no UID mapping. All mount types are available.

### Extract-and-run mode

When `--extract=force` is set, or when `--extract=allow` is set and `unshare(2)`
fails, fuselage skips `enter_namespace()` entirely and extracts archives into the
procdir without entering a mount namespace. This mode works in locked-down
containers that block `unshare` or disable unprivileged user namespaces. It is
the same escape-hatch strategy used by AppImage's `--appimage-extract-and-run`
flag for running in containers where FUSE is unavailable.

**Known limitations in this mode:**

- **`--static` archives are not read-only.** In namespace mode the static
  directories are bind-mounted read-only by the kernel (`EROFS` on any write
  attempt). In extract-and-run mode there is no mount namespace and therefore
  no bind-remount-ro. File permissions (`chmod -R a-w`) cannot substitute: the
  child process runs as the same user who owns the extracted files and can
  restore write permission at will; root ignores file permissions entirely via
  `CAP_DAC_OVERRIDE`. See [decision 0004](../decisions/0004-extract-mode-no-readonly-enforcement/0004-extract-mode-no-readonly-enforcement.md).
- **Fixed-path mounts are unavailable.** Archives named `/run/fuselage/NAME`
  require a private mount namespace to be safe. Requesting one in extract-and-run
  mode is a hard error.
- **No tmpfs overlay on the procdir.** The procdir lives on the real filesystem
  rather than on a private tmpfs; it is still cleaned up on exit.

## Error handling

- If `~/.fuselage/` does not exist it is created (mode 0700).
- If `~/.fuselage/` exists but is not owned by the caller, fuselage aborts.
- If an archive cannot be extracted or mounted, fuselage aborts before exec.
- If archive names collide, fuselage aborts with a message suggesting `NAME:`.
- If `--run` path is not found or is ambiguous, fuselage aborts with details.
- If `--run` target is not executable, fuselage aborts.
- If a fixed-path mount is requested in unprivileged mode, fuselage aborts with
  a message indicating that setuid-root installation is required.
- If a fixed-path mount is requested with `--extract=force` or when
  `--extract=allow` falls back to extract-and-run, fuselage aborts with a
  message explaining the incompatibility.
- If the mount namespace cannot be created and `--extract=deny` (the default)
  is in effect, fuselage aborts with a diagnostic.

## Examples

```bash
# Just an ephemeral scratch space.
fuselage bash
ls "$FUSELAGE_TMPDIR"   # empty, writable, gone when bash exits

# Run a build with a cached SDK and a throwaway working copy.
fuselage --static=sdk:toolchain.zip --dynamic=src:source.zip \
    -- make -C "$FUSELAGE_DYNAMIC/src" -j4

# Run an executable from inside an archive.
fuselage --dynamic=app:my-app.zip --run bin/server --port 8080

# Build a portable Python venv at a fixed path, capture as squashfs.
fuselage --dynamic-empty=/run/fuselage/myapp -- bash -c '
  uv venv /run/fuselage/myapp/.venv
  uv pip install -r requirements.txt
  mksquashfs /run/fuselage/myapp myapp.sfs
'

# Run the captured venv (squashfs loop-mounted at the same fixed path).
fuselage --static=/run/fuselage/myapp:myapp.sfs -- \
  /run/fuselage/myapp/.venv/bin/python -m myapp

# Bundle the above into a single self-executing binary.
fuselage-bundle --archive=myapp.sfs --output=myapp \
  -- --static=/run/fuselage/myapp:/proc/self/exe \
     --run /run/fuselage/myapp/.venv/bin/python \
     -- -m myapp
```
