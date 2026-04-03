# fuselage -- ephemeral virtual filesystems for processes

## Overview

`fuselage` runs a command with ephemeral, namespace-private filesystems
derived from zip archives. It requires no containers, no daemons, and no
manual cleanup. Archives are unpacked -- never mounted as zip -- so the
command sees a normal directory tree.

There is no isolation: the process keeps its normal environment, PID space,
network, and UID. The only thing `fuselage` adds is a private mount
namespace so that (a) the ephemeral mounts are invisible to other processes
and (b) they vanish automatically when the command exits.

## Usage

```
fuselage [OPTIONS...] [--] COMMAND [ARG...]
fuselage [OPTIONS...] --run PATH [ARG...]
```

All options are optional. With no `--static` or `--dynamic` flags,
fuselage simply provides an ephemeral tmpdir at `$FUSELAGE_TMPDIR`.

### `--dynamic=[NAME:]FILE`

Extract `FILE` into a fresh, mutable directory under `$FUSELAGE_DYNAMIC/`.
The extraction is private to this invocation and is discarded when the
command exits.

`FILE` must be either:

- a zip file (detected by magic bytes `PK\x03\x04`), or
- a text file containing base64-encoded zip data.

`NAME` is optional. If omitted, it is derived from the filename by
stripping the extension (e.g. `my-data.zip` -> `my-data`). The archive
is extracted into `$FUSELAGE_DYNAMIC/NAME/`.

### `--static=[NAME:]FILE`

Like `--dynamic`, but:

- The extraction is cached persistently under `~/.fuselage/cache/`
  using a content hash (SHA-256 prefix) of `FILE`. Subsequent runs
  with the same file skip extraction entirely.
- The directory is mounted **read-only** inside the namespace.
- The archive appears at `$FUSELAGE_STATIC/NAME/`.

### `--run PATH [ARG...]`

Search all extracted archives (both static and dynamic) for `PATH`.
If found in exactly one archive, execute it with the remaining
arguments. Errors if:

- `PATH` is not found in any archive.
- `PATH` is found in multiple archives (ambiguous).
- The found file is not executable.

`--run` is an alternative to `-- COMMAND`; use one or the other.
Requires at least one `--static` or `--dynamic` archive.

### Archive name uniqueness

All archive names must be unique across `--static` and `--dynamic`.
If two archives would have the same derived name (e.g. both called
`foo.zip`), use the `NAME:` prefix to disambiguate:

```
fuselage --static=cached:foo.zip --dynamic=fresh:foo.zip -- ...
```

### `COMMAND [ARG...]`

The command to execute. It runs as the calling user with full access
to the normal filesystem, plus the additional mounts.

## Directory layout

```
~/.fuselage/
  procdirs/<pid>/              per-process ephemeral root (tmpfs)
    tmp/                       scratch space ($FUSELAGE_TMPDIR)
    dynamic/NAME/              --dynamic extractions
    static/NAME/               --static read-only bind-mounts
  cache/<sha256-prefix>/       --static persistent cache
```

- `procdirs/<pid>/` is overlaid with a tmpfs inside the mount
  namespace. Other processes see it as an empty directory. It is
  removed on exit.

- `cache/<sha256-prefix>/` lives on the real filesystem and persists
  across invocations. A sentinel file at `cache/<sha256-prefix>.complete`
  marks a finished extraction.

## Environment variables

| Variable | Set when | Value |
|---|---|---|
| `FUSELAGE_TMPDIR` | Always | `~/.fuselage/procdirs/<pid>/tmp` |
| `FUSELAGE_DYNAMIC` | Any `--dynamic` | `~/.fuselage/procdirs/<pid>/dynamic` |
| `FUSELAGE_STATIC` | Any `--static` | `~/.fuselage/procdirs/<pid>/static` |

These are inherited by child processes.

Access individual archives by name:
`$FUSELAGE_DYNAMIC/my-data/config.json`,
`$FUSELAGE_STATIC/sdk/bin/gcc`, etc.

## Privilege model

`fuselage` needs to call `mount(2)`, which requires `CAP_SYS_ADMIN`.
There are two ways to get it:

### User namespace mode (default)

When run as a normal user, fuselage uses `unshare --user --mount
--map-root-user` to create a user namespace where the caller is
mapped to root. This requires no installation and no privileges,
but has a caveat: inside the namespace, the process sees itself as
uid 0 (root). Files created on real filesystems are owned by the
real user, but tools like `id` and `ls` show uid 0. `sudo` will
not work inside the namespace.

This is the only mode available to the bash implementation, since
bash drops setuid privileges on startup.

### Setuid binary mode (future, requires C implementation)

A compiled C binary can be installed setuid-root. The privileged
window is minimal:

1. Save the caller's real UID/GID.
2. `unshare(CLONE_NEWNS)` -- create a private mount namespace.
3. Mount a tmpfs and extract/mount archives.
4. `chown` mount roots to the real user.
5. `setresuid(real_uid, real_uid, real_uid)` -- permanently and
   irreversibly drop all privileges.
6. `exec` the command as the original user.

This preserves normal UID semantics: the process sees its real UID,
`sudo` works, and file ownership is straightforward. This mode
requires a C binary because the kernel ignores setuid bits on
scripts (bash, Python, etc.) as a security measure.

### Running as root

If the caller is already root (e.g. via `sudo fuselage ...`),
fuselage skips the user namespace and uses a plain mount namespace.
No UID mapping caveats apply.

## Archive format

Both `--static` and `--dynamic` accept two file types:

1. **Zip files** -- detected by the magic bytes `PK\x03\x04` at offset 0.
   Extracted with `unzip`.

2. **Base64-encoded zip files** -- any file that does not start with
   zip magic bytes. The entire file content is decoded with `base64 -d`
   and then extracted as a zip. This supports embedding archives in
   text-based formats (scripts, heredocs, etc.).

## Future: herescript integration

`fuselage` is designed to compose with `herescript`. A herescript file
may embed one or more base64-encoded zip archives and invoke `fuselage`
to make them available as directories. The shebang/marker format for
this integration is deferred to a subsequent specification.

## Examples

```bash
# Just an ephemeral scratch space
fuselage bash
ls "$FUSELAGE_TMPDIR"   # empty, writable, gone when bash exits

# Run a build with a cached SDK and a throwaway working copy
fuselage --static=sdk:toolchain.zip --dynamic=src:source.zip \
    -- make -C "$FUSELAGE_DYNAMIC/src" -j4

# Run an executable from inside an archive
fuselage --dynamic=app:my-app.zip --run bin/server --port 8080

# Multiple archives
fuselage \
    --static=libs:libs.zip \
    --static=assets:assets.zip \
    --dynamic=build:build-seed.zip \
    -- ./run-pipeline.sh
```

## Error handling

- If `~/.fuselage/` does not exist, create it (mode 0700).
- If `~/.fuselage/` exists but is not owned by the caller, abort.
- If a zip file cannot be extracted, abort before exec-ing the command.
- If archive names collide, abort with a message suggesting NAME: prefix.
- If `--run` path is not found or is ambiguous, abort with details.
- If `--run` target is not executable, abort.
- If the mount namespace cannot be created, abort with a diagnostic
  suggesting either setuid installation or user namespace support.
- Stale `procdirs/<pid>/` directories (where the pid no longer exists)
  may be cleaned up opportunistically on startup.
