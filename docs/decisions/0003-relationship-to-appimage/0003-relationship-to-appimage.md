# 0003 - Relationship to AppImage, 2026-06-15

## Issue

fuselage-bundle produces a single self-executing ELF binary with a squashfs
image appended after it — the same physical file layout as AppImage Type 2. The
obvious question, asked repeatedly, is: *why not just use AppImage?* This record
documents the deliberate differences in mechanism and the reasoning behind them,
so the design intent is not lost.

This is a post-hoc record: the design was arrived at incrementally (issues #7,
#8, #9) rather than as a single choice between AppImage and a clean-sheet
alternative. There were therefore no formal options weighed, so the
options/pros-and-cons sections of the template are omitted per the project
guidance.

## The shared concept

Both tools produce a distributable, self-contained executable file with the
layout:

```
[ ELF stub (small launcher)     ]
[ zero padding -> page boundary  ]
[ squashfs image                ]
```

The leading bytes are a real, runnable ELF; the application payload is a
squashfs concatenated after it. The OS executes the ELF, which locates the
appended squashfs relative to itself via `/proc/self/exe`. The page-aligned
offset exists in both for the same reason: so the filesystem image begins on a
mountable boundary. Conceptually the two artifacts are the same family.

The differences are entirely in what the stub does at runtime and the privileges
and kernel features that requires.

## The key difference: self-mounting vs self-launching

**AppImage is self-mounting.** Its stub embeds a FUSE runtime (squashfuse) and
mounts its own payload, in userspace, to an unpredictable per-run temp path
(`/tmp/.mount_XXXX`). It is fully standalone: nothing need be installed on the
host beyond a working FUSE.

**fuselage-bundle is self-launching.** Its stub
([src/bin/stub_template.c](../../../src/bin/stub_template.c)) is ~120 lines whose
entire job is argv-munging and one `execvp` of `fuselage` found on `PATH`
(see `src/bin/stub_template.c:116`). The stub does not know how to read squashfs
at all; it delegates everything to an installed `fuselage`. fuselage then mounts
or extracts the embedded squashfs, which it locates by parsing the ELF headers
rather than scanning for magic bytes (`src/archive.rs`, `detect_elf_squashfs`).

This single distinction — delegating to an installed `fuselage` instead of
carrying a mount runtime — drives every other difference below.

## Consequences of the choice

### Gained: a fixed, predictable mount path

Because fuselage mounts the payload at a fixed path (`/run/fuselage/NAME`,
`src/archive.rs`, `parse_mount_name`), an environment can bake absolute paths and
rely on the contents appearing at exactly that location at runtime. This is the
decisive advantage for **pre-built Python venvs**, whose shebangs, `pyvenv.cfg`,
and `.pth` files hardcode the venv's absolute path. AppImage's random temp
mountpoint forces everything inside to be position-independent, which is exactly
the thing that makes bundling a venv painful. (See #7.)

### Gained: no hard FUSE dependency, and a userspace fallback

fuselage has two runtime paths (`src/main.rs`):

- **Privileged / setuid-root** -> kernel loop-device mount with `lo_offset`
  (`src/procdir.rs`, `loop_mount_sfs`). Needs `CAP_SYS_ADMIN` and a loop device.
- **Unprivileged** -> userspace extraction via the `backhand` library
  (`src/archive.rs`, `extract_squashfs`). Needs no FUSE, no loop device.

Neither path requires `/dev/fuse`. AppImage, by contrast, has a hard FUSE
dependency and falls back only via the manual, off-to-the-side
`--appimage-extract`. fuselage's userspace extraction is a first-class automatic
code path selected by the same euid check that drives the rest of its behaviour.

### Gained: namespace isolation and auto-cleanup

The payload is mounted inside fuselage's private, ephemeral mount namespace
(`src/namespace.rs`), and the procdir is cleaned up automatically on exit. An
AppImage mount is just a mounted directory with no isolation.

### Gained (potential): composability

fuselage's multi-archive model leaves room to compose several squashfs images in
one invocation — something AppImage's single-payload model does not address.

### Lost: full self-containment

This is the real cost. An AppImage is double-click-and-go with zero install. A
fuselage bundle is a thin launcher that **requires `fuselage` already installed
on the host**; the stub exits with a clear error if it is not on `PATH`. The
privilege/mount machinery is deliberately pushed out of every bundle and into
one shared, auditable, optionally-setuid binary.

### Privilege model: why fuselage incurs UID mapping that AppImage does not

A standard AppImage never calls `mount(2)`; FUSE lets an ordinary user present a
filesystem without acquiring any capability, so there is no user namespace and
no UID map.

fuselage's unprivileged path *does* perform a real kernel mount (the
bind-remount-ro, and historically the payload mount). To get `CAP_SYS_ADMIN` as
an unprivileged user it creates a user namespace (`CLONE_NEWUSER`), and the
kernel requires a UID map to be written before that namespace is usable
(`src/namespace.rs:36-39`). The UID mapping is therefore not a feature but the
unavoidable entry ticket to unprivileged mounting. In setuid-root mode fuselage
already holds the capability and does no UID mapping at all.

## Container behaviour

Both tools can fail inside containers for the same *class* of reason: the mount
mechanism each relies on is among the most commonly stripped container
capabilities (FUSE / `/dev/fuse` for AppImage; loop devices and `CAP_SYS_ADMIN`
for fuselage's privileged path). fuselage's unprivileged extraction avoids those
but still depends on unprivileged user namespaces being available, because
`enter_namespace()` is currently called unconditionally (`src/main.rs`). A fully
userspace extract-and-run mode that skips the namespace entirely — the closest
analogue to AppImage's extract-and-run — is proposed in #12, with the explicit
caveat that it would break fixed-path images such as Python venvs.

## Additional notes

This record consolidates the reasoning behind issues #7 (fixed mount paths), #8
(fuselage-bundle), #9 (ELF+squashfs input), and #12 (extract-and-run mode). The
recurring summary worth keeping in mind:

> AppImage is a self-mounting bundle (carries its own FUSE runtime, mounts to a
> random path, fully standalone). fuselage-bundle is a self-launching bundle with
> the same on-disk layout, but it delegates mounting to an installed `fuselage` —
> trading self-containment for a fixed `/run/fuselage/NAME` mount path (which
> makes pre-built venvs work), a FUSE-free unprivileged fallback, namespace
> isolation, and auto-cleanup.
