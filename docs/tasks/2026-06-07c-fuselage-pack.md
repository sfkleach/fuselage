# Task: Implement fuselage-pack — bundle a squashfs and fuselage command into a single self-executing ELF binary

## Overview

This task implements Issue https://github.com/sfkleach/fuselage/issues/8

## Overview

`fuselage-pack` is a companion tool that takes a squashfs archive and a fuselage
invocation and produces a single self-executing ELF binary. The resulting file
can be distributed and run directly — it locates `fuselage` on `PATH` and
invokes it with the embedded squashfs and baked-in arguments.

## Output binary structure

```
[ ELF stub (statically linked)  ]
[ padding to page alignment     ]
[ squashfs image                ]
```

The stub is a small statically-linked binary. The squashfs is appended at a
page-aligned offset after the ELF sections. This is the same physical layout as
AppImage Type 2.

## Invocation

```
fuselage-pack [FUSELAGE_OPTIONS...] --archive=SQUASHFS --output BINARY_FILE 
```

Example:

```bash
fuselage-pack \
  --static=/run/fuselage/myapp:/proc/self/exe \
  --run /run/fuselage/myapp/.venv/bin/python -m myapp
  --archive=myapp.sfs \
  --output=myapp
```

The resulting `myapp` binary, when executed as `myapp ARGS...`, does the equivalent of:

```bash
exec fuselage \
  --static=/run/fuselage/myapp:/proc/self/exe \
  --run /run/fuselage/myapp/.venv/bin/python -m myapp \
  -- ARGS...
```

## Dependency: ELF preprocessing in fuselage

For this to work, fuselage must recognise an ELF binary as a valid archive
argument — detecting the ELF magic, scanning forward for squashfs magic bytes,
recording the byte offset, and treating the remainder identically to a
standalone squashfs. In privileged mode this means passing `lo_offset` to the
loop device; in unprivileged mode it means seeking to the offset before handing
the data to `backhand`.

This is a general preprocessing step rather than a new archive format: the
underlying archive is still squashfs; the ELF is just a carrier.

## Stub behaviour

The stub is compiled into `fuselage-pack` as a binary blob and appended to the
squashfs at pack time. At runtime it:

1. Resolves its own absolute path via `/proc/self/exe`
2. Substitutes the resolved path for `@self` in the baked-in argument list
3. `execvp`s `fuselage` with the substituted arguments, passing through `argv[1..]` from the user

If `fuselage` is not found on `PATH`, the stub exits with a clear error message.

## Complete Python packaging workflow

```bash
# 1. Build
fuselage --dynamic-empty=/run/fuselage/myapp -- bash -c '
  uv sync --project /path/to/myproject
  mksquashfs /run/fuselage/myapp myapp.sfs -noappend -quiet
'

# 2. Pack
fuselage-pack \
  --static=/run/fuselage/myapp:@self \
  --output myapp \
  myapp.sfs \
  -- /run/fuselage/myapp/.venv/bin/python -m myapp

# 3. Distribute and run
./myapp --some-arg   # finds fuselage on PATH, mounts squashfs, runs Python
```

## Relationship to AppImage

The physical file layout and the `LO_FLAGS_OFFSET` loop-mount mechanism are the
same as AppImage Type 2. The key differences are:

- The stub invokes `fuselage` rather than mounting via FUSE, giving access to fuselage's setuid privilege model and fixed-path mounting
- The fixed-path mounting (`/run/fuselage/NAME`) allows pre-built venvs with correct hardcoded paths, without FUSE being required on the target
- Multiple archives can be composed (if future `fuselage-pack` variants support embedding more than one squashfs)

## Implementation notes

- `fuselage-pack` is likely a separate binary in the same Cargo workspace rather than a subcommand, keeping the runtime binary lean
- The stub must be statically linked and architecture-specific; `fuselage-pack` ships pre-compiled stubs for each supported target (x86_64, aarch64)
- The baked-in argument list can be stored as a null-terminated string in a dedicated ELF section in the stub, written by `fuselage-pack` before appending the squashfs
- Depends on the ELF preprocessing feature being implemented in fuselage first