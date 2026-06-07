# Task: Fixed mount paths under /run/fuselage/ for bundling relocatable environments

## Problem

Fuselage mounts `--static` archives under `~/.fuselage/procdirs/<pid>/static/<name>/`. This path is unknowable at package-build time, which makes it impossible to bundle a pre-built Python virtual environment: a venv bakes absolute paths into `pyvenv.cfg` and `bin/` script shebangs at creation time, and those paths must match the mount location at runtime.

## Proposed solution

Allow `--static` and `--dynamic` to accept an absolute path under `/run/fuselage/` as the mount point name, in place of the usual relative name:

```
--static=/run/fuselage/NAME:FILE    # loop-mount FILE read-only at /run/fuselage/NAME
--dynamic=/run/fuselage/NAME:FILE   # extract FILE into writable /run/fuselage/NAME
```

Additionally, add a new flag for the build-time use case where no seed archive is needed:

```
--dynamic-empty=/run/fuselage/NAME  # create an empty writable directory at /run/fuselage/NAME
```

### Why /run/fuselage is the right location

- `/run` is semantically correct: it is the standard location for runtime state managed by system-level programs.
- `/run` is a tmpfs recreated at boot, so `/run/fuselage` is created on first use after each boot — no installer ceremony, no `tmpfiles.d` snippet.
- Fuselage is setuid-root, giving it a privilege window in which it can `mkdir /run/fuselage` and the specific subdirectory before dropping back to the real uid. No extra privileges are required beyond what fuselage already has.
- Concurrent fuselage invocations do not conflict: each runs in its own private mount namespace, so mounting different archives at `/run/fuselage/NAME` in separate invocations is completely independent.

### Security restriction

The absolute path name must:
- Start with `/run/fuselage/`
- Have exactly one further path component (e.g. `/run/fuselage/myapp` is valid; `/run/fuselage/a/b` is not)

This prevents fuselage from being used to shadow arbitrary system directories. The restriction is enforced at argument-parse time. The `/run/fuselage/` prefix is the only allowed root for now, leaving the door open for future extension.

### Fixed-path mounts are privileged-mode only

Creating directories under `/run/` and loop-mounting require `CAP_SYS_ADMIN`. Fixed-path mounts are therefore only available when fuselage is running setuid-root. Attempting to use them in unprivileged (user-namespace) mode should produce a clear error.

## Workflow

### Building a portable Python app

```bash
# Build: create the venv at the fixed path, capture as squashfs
fuselage --dynamic-empty=/run/fuselage/myapp -- bash -c '
  uv venv /run/fuselage/myapp/.venv
  uv pip install -r requirements.txt
  cp -r src/ /run/fuselage/myapp/
  mksquashfs /run/fuselage/myapp ./myapp.sfs
'

# Run: mount the squashfs at the same fixed path
fuselage --static=/run/fuselage/myapp:myapp.sfs -- \
  /run/fuselage/myapp/.venv/bin/python -m myapp
```

The `mksquashfs` output is written to the real filesystem (the namespace only shadows `/run/fuselage/myapp`), so the `.sfs` survives after the build invocation exits. At runtime, the venv's hardcoded paths resolve correctly because the squashfs is always mounted at the same location.

## Implementation sketch

1. **Argument parsing** (`archive.rs`): extend `validate_name` to recognise absolute paths matching `/run/fuselage/<single-component>` and pass them through; reject any other absolute path with a clear error message.
2. **Privilege window** (`main.rs`): before `seteuid` drops to the real uid, `mkdir -p /run/fuselage` and the specific subdirectory if they do not exist.
3. **Static mount phase** (`main.rs`): when the name is an absolute path, loop-mount (or extract+bind-ro) directly onto that path rather than under `$FUSELAGE_STATIC/`.
4. **Dynamic mount phase** (`main.rs`): likewise, extract into the absolute path rather than under `$FUSELAGE_DYNAMIC/`. For `--dynamic-empty`, simply `mkdir` the target and skip archive extraction entirely.
5. **`FUSELAGE_STATIC` / `FUSELAGE_DYNAMIC`**: these env vars are set only when at least one relative-name archive is present. Fixed-path archives do not affect them.