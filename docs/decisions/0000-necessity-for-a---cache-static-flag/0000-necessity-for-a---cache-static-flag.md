# 0000 - Necessity for a --cache-static flag, 2026-04-04

## Issue

`fuselage --static` extracts an archive into a private, ephemeral directory that
vanishes when the command exits. This is intentional: the archive contents are
never written outside the controlled invocation.

A caching mechanism was introduced in Step 5 to avoid re-extracting `--static`
zip archives on every run. The cache stores a squashfs image (or an extracted
directory if `mksquashfs` is unavailable) in `~/.fuselage/cache/`, keyed by
SHA-256 content hash. On a cache hit the archive is loop-mounted or bind-mounted
from the cache instead of being extracted again.

The decision was whether to make this caching opt-in (via a flag) or opt-out
(enabled by default, with a flag to disable it).

## Factors

- **Confidentiality of archive contents.** A `--static` archive may contain
  sensitive material — credentials, proprietary code, personal data. The user
  passes the archive knowing that `fuselage` will expose it to a subprocess
  inside a private namespace and then discard it. If caching is enabled by
  default, a persistent copy of the archive's extracted contents is written to
  `~/.fuselage/cache/` without the user explicitly requesting that. This may
  violate data-handling policies or user expectations, and creates a durable
  attack surface that did not exist in the uncached path.

- **Principle of least surprise.** Users who do not read the documentation in
  detail should not find that `fuselage` has quietly persisted data from a
  `--static` archive to their home directory. The uncached behaviour (extract,
  use, discard) is simple to reason about and auditable. Caching introduces
  state that requires ongoing management.

- **Performance benefit is situational.** Caching only improves performance
  when the same archive is used repeatedly across multiple invocations. In
  one-shot or scripted contexts (CI pipelines, single runs) there is no benefit
  and only the drawback of written state. Opt-in caching means the performance
  benefit is obtained exactly when the user has decided it is appropriate.

- **Cache location is predictable and user-controlled.** Because the cache key
  is a SHA-256 content hash, a compromised cache entry is tied to a specific
  archive file. This is a reasonable model, but it should be the user's
  deliberate choice to create it.

## Options and Outcome

**Option 1 (chosen): `--cache-static` opt-in flag.**
Caching is disabled by default. The user must pass `--cache-static` explicitly
to enable persistent caching of zip archives as squashfs images.

**Option 2: Cache enabled by default, `--no-cache-static` to disable.**
Caching is always active unless the user remembers to turn it off.

The decision was **Option 1**. The security and confidentiality argument
outweighs the convenience of defaulting to the faster path. Users who want
caching know they want it and can opt in.

## Consequences

- Without `--cache-static`, running `fuselage --static=archive.zip CMD` writes
  nothing outside the tmpfs. The archive contents are held only in the private
  mount namespace and are gone when `CMD` exits.

- With `--cache-static`, a squashfs image (or directory) is written to
  `~/.fuselage/cache/<sha256-16>.sfs` on first use and reused on subsequent
  runs. The user takes on the responsibility of managing this cache (expiry,
  access controls, deletion of sensitive material).

- `.sfs` files passed directly to `--static` are never cached regardless of
  the flag — they are already in their optimal form and are used (loop-mounted
  or extracted) directly from the path provided.

## Pros and Cons of Options

### Option 1 — `--cache-static` opt-in (chosen)

- **Pro**: No persistent data is written without explicit user consent.
- **Pro**: Safe by default; complies naturally with data-handling policies that
  prohibit uncontrolled copies of sensitive material.
- **Pro**: Simpler mental model — without the flag, fuselage leaves no trace.
- **Con**: Users who always want caching must remember to pass the flag, or
  wrap `fuselage` in a shell alias/script.

### Option 2 — cache enabled by default

- **Pro**: Faster out of the box for repeated invocations with the same archive.
- **Con**: Silently writes archive contents to disk without the user's knowledge.
- **Con**: Creates a durable copy that persists after the subprocess has exited,
  potentially indefinitely if the cache is never evicted.
- **Con**: Requires users in security-sensitive contexts to actively remember to
  disable caching — a burden that is easy to forget.

## Additional Notes

The same reasoning would apply to any future `--dynamic` caching. Dynamic
archives are writable by design, making their contents even more sensitive to
inadvertent persistence; any caching of dynamic archives should also be
strictly opt-in.
