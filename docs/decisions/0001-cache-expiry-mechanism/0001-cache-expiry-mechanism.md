# 0001 - Cache Expiry Mechanism, 2026-04-04

## Issue

The `--cache-static` cache at `~/.fuselage/cache/` grows without bound. Once
a zip archive has been converted to a squashfs image and stored there, nothing
removes it automatically. Over time the cache will accumulate stale entries for
archives that are no longer in use.

A mechanism is needed to evict entries that have not been used recently, while
leaving entries that are still in active use untouched.

## Factors

- **Single binary, no installation dependencies.** Fuselage is designed to be
  a self-contained binary with minimal installation requirements. Requiring a
  companion daemon would add an installation step, a service-management
  dependency (systemd, cron, or similar), and a failure mode when the daemon is
  not running. The cache expiry mechanism must work without any external process
  being present. (A user who wants a daemon-based approach can layer one on top,
  but fuselage itself must not require it.)

- **No daemon.** Fuselage is a one-shot CLI tool; there is no persistent process
  to perform background housekeeping. Any reaper must be spawned on demand.

- **atime is unreliable.** Many Linux filesystems are mounted with `noatime` or
  `relatime` for performance. Relying on the kernel to update access times when
  an `.sfs` file is opened is not portable. Timestamps used for expiry must be
  written explicitly via `utimes()`/`utimensat()`.

- **Periodic refresh is unnecessary.** The original proposal suggested updating
  the last-access time periodically during a run so that long-lived invocations
  would not expire their own cache entries. In practice, fuselage processes are
  short-lived (seconds to minutes); no single run will outlast a reasonable expiry
  window (days to weeks). A single explicit touch at startup is sufficient.

- **Concurrent safety.** Multiple fuselage processes may use the same cache entry
  simultaneously. A reaper spawned by one process must not delete an entry that
  another process has just touched but not yet mounted.

- **Interrupted builds.** A cache miss starts by extracting to a temp dir, then
  calling `mksquashfs`, then writing the `.complete` sentinel. If the process is
  killed mid-build, the partial `.sfs` file exists but the `.complete` sentinel
  does not. These orphaned files must also be cleaned up.

## Options and Outcome

**Option 1 (chosen): Explicit mtime touch + mtime-based reaper with recency guard.**

**Option 2: atime-based (filesystem-native).**
Rely on the kernel to update access times when the `.sfs` file is opened.

**Option 3: Reference-counting lock files.**
On use, create `<hash>.lock.<pid>`; on exit, remove it. Reaper skips entries
that have any lock file.

**Option 4: Central index file.**
Maintain `~/.fuselage/cache/index.json` recording per-entry metadata.

The decision was **Option 1**. See Pros and Cons below.

## Mechanism (Option 1)

### Marking a cache entry as recently used

When fuselage starts and finds a cache hit (the `.complete` sentinel exists),
it explicitly updates the sentinel's modification time to the current time using
`utimes()` before proceeding to mount. This is a lightweight write that does not
depend on filesystem mount options.

### Reaping stale entries at exit

When a fuselage process exits and the cache directory exists, it spawns a
short-lived background process (double-fork so the parent does not need to wait)
to reap stale entries. The reaper:

1. Scans `~/.fuselage/cache/` for `.complete` sentinel files.
2. For each sentinel, reads its modification time.
3. Skips any sentinel whose mtime is within the last **60 seconds** (recency
   guard — protects entries touched by a concurrent process that has not yet
   mounted them).
4. Removes entries whose mtime is older than the **expiry threshold** (default:
   30 days). Removal means deleting the `.sfs` file (if present), the extracted
   directory (if present), and the `.complete` sentinel.
5. Additionally removes any `.sfs` file that has no corresponding `.complete`
   sentinel and whose mtime is older than **1 hour** (orphaned partial builds).

The reaper runs with normal user permissions and performs no mounts. Errors
during reaping are silently ignored — the cache is a performance aid, not a
critical resource.

### Configuration

The expiry threshold will be configurable via the environment variable
`FUSELAGE_CACHE_MAX_AGE_DAYS` (integer, default `30`). Setting it to `0`
disables automatic reaping.

## Consequences

- Cache entries are kept alive as long as they are used at least once within
  the expiry window. An entry used today will survive for another 30 days from
  today.
- There is a small window (≤ 60 seconds) during which an entry is safe from
  reaping even if it was just written. This is conservative but harmless.
- No persistent daemon, no lock files, no index file, no threads.
- On filesystems with `noatime`, the mechanism still works because we write
  mtimes explicitly.

## Pros and Cons of Options

### Option 1 — Explicit mtime touch + recency guard (chosen)

- **Pro**: Works on all filesystem mount options (writes mtime explicitly).
- **Pro**: No persistent state beyond the files already in the cache directory.
- **Pro**: No threads, no lock files, no index file.
- **Pro**: Handles orphaned partial builds.
- **Con**: Reaper spawned at every exit when cache dir exists, even if nothing
  needs evicting. Cost is negligible (stat + readdir + a few unlinks).

### Option 2 — atime-based

- **Pro**: Zero extra code — kernel updates atime on open.
- **Con**: Broken on `noatime`/`relatime` mounts, which are extremely common.
- **Con**: Requires reading atime rather than mtime; less predictable semantics.

### Option 3 — Reference-counting lock files

- **Pro**: Precise knowledge of which entries are live at any moment.
- **Con**: Lock files must be cleaned up on abnormal exit (SIGKILL, crash).
  Stale lock files would permanently protect entries from eviction.
- **Con**: Requires logic to detect and remove stale locks.

### Option 4 — Central index file

- **Pro**: Rich metadata; easy to add per-entry notes (origin path, size, etc.).
- **Con**: Requires file locking for concurrent updates — significant complexity.
- **Con**: Index can become inconsistent if a process is killed while writing.
