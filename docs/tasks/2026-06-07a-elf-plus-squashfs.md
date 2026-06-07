# Task: Support ELF+squashfs as a valid archive input

## Overview

This is an implementation of https://github.com/sfkleach/fuselage/issues/9 but
with a slight change in implementation of how we find the offset of the squashfs.

When fuselage receives an archive argument whose file begins with ELF magic
(`\x7fELF`), it should search for the end of the last segment, and treat the
data from that offset onwards identically to a standalone squashfs file.

This is a preprocessing step, not a new archive format — the underlying archive
is still squashfs. The ELF is a transparent carrier.

### Motivation

This is the prerequisite for `fuselage-pack` (issue #8), which produces
self-executing ELF binaries with an embedded squashfs. It also provides
incidental interoperability with existing AppImage files, which use the same
physical layout.

### Why squashfs and not zip

The loop device driver supports `lo_offset` (`LO_FLAGS_OFFSET`), which allows a
squashfs to be mounted directly from an arbitrary byte offset within a larger
file. No extraction to a temp file is needed. This is precisely what makes the
ELF+squashfs combination useful — the ELF prefix is invisible to the kernel's
squashfs driver.

Zip does not benefit from the same treatment: fuselage must extract zip archives
to a directory regardless, so embedding a zip inside an ELF would add complexity
with no compensating advantage.

## Implementation

### Detection

Extend `detect_format` in `archive.rs`.  AppImage Type 2 specifically places the
squashfs immediately after the last loadable segment — you can compute that from
the program header table (PT_LOAD segments), taking the highest p_offset +
p_filesz and rounding up to the squashfs block alignment (4096 bytes). 

N.B. If the raw end is already aligned there is no need to round up.

If no squashfs magic is found immediately after the ELF, return an error
(unrecognised format).

A new internal type (not a public `ArchiveFormat` variant) can carry the offset
alongside the squashfs format indicator, keeping the existing dispatch logic
largely unchanged.

### Privileged mode (loop mount)

Pass `lo_offset` to the loop device when attaching the file. The kernel squashfs
driver reads from the loop device, which transparently handles the offset. No
changes needed to the squashfs mount logic beyond threading the offset through.

### Unprivileged mode (backhand extraction)

The `backhand` crate's `FilesystemReader` must be initialised from a reader that
has been seeked to the squashfs offset within the ELF file. Verify that
`backhand` correctly handles a reader that does not start at byte zero — if not,
a thin wrapper that seeks on construction is sufficient.

## Relationship to AppImage

An existing AppImage Type 2 file passed as a fuselage archive argument will work
correctly once this feature is implemented. This is not a primary goal but is a
natural consequence of sharing the same file layout.
