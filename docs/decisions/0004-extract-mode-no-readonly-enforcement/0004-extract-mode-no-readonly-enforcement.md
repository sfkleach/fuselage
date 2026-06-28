# 0004 - Extract-and-run mode: read-only not enforced, 2026-06-27

## Issue

**Fork.** When implementing extract-and-run mode (`--extract=allow`/`--extract=force`),
which skips user namespaces entirely, should `--static` archives have their
read-only property enforced? Two mechanisms were considered:

1. Set restrictive file permissions (`chmod -R a-w`) after extraction.
2. Document the absence of enforcement as a known limitation.

## Factors

- In namespace mode (setuid-root and unprivileged), `--static` archives are
  bind-mounted read-only. The kernel enforces this at the VFS layer — any
  write attempt returns `EROFS` regardless of the caller's identity.
- In extract-and-run mode there is no mount namespace and therefore no
  bind-remount-ro. The extracted files live on the real filesystem and are
  owned by the invoking user.
- The child process runs as the same user who owns the extracted files.
  File permissions can be set, but the owner can always restore write
  permission (`chmod u+w`), so the constraint is trivially bypassable.
- Root ignores DAC permissions (`CAP_DAC_OVERRIDE`), so permission-based
  enforcement provides no protection if the child gains elevated privilege
  via a setuid helper.
- The error code differs: bind-remount-ro returns `EROFS`; `chmod a-w`
  returns `EACCES`. Some software branches on this distinction.

## Decision

Do not attempt to enforce read-only permissions in extract-and-run mode.
Document the absence as a known limitation in `README.md` (linked from `--help`),
`docs/SPEC.md`, and `CHANGELOG.md`.

## Consequences

The read-only guarantee offered by `--static` in namespace mode does not carry
over to extract-and-run mode. Users who rely on immutability of static archives
must use namespace mode (setuid-root or unprivileged). Extract-and-run mode
is intended for environments where no namespace is available at all; in those
environments, losing the read-only guarantee is an accepted cost of being able
to run at all.

The extractions remain ephemeral: the procdir is cleaned up on exit regardless
of mode, so any modifications the child makes to the extracted contents are
discarded when the invocation ends.

## Options and Outcome

**Option 1 chosen:** Do not enforce — document the limitation.

**Option 2 rejected:** `chmod -R a-w` — appears to restrict writes but is
trivially bypassed by the file owner (the invoking user), which is exactly
the identity the child process runs as.

## Pros and Cons of Options

### Option 1: No enforcement; document as limitation

- Pro: Honest — the guarantee is accurately documented rather than falsely implied.
- Pro: No false sense of security from a permission the file owner can trivially undo.
- Pro: Avoids a mismatched error code (`EACCES` vs `EROFS`) that could confuse software.
- Con: `--static` no longer implies read-only in all operating modes.

### Option 2: `chmod -R a-w` after extraction

- Pro: Prevents casual accidental writes.
- Con: Bypassable by the file owner with a single `chmod u+w`.
- Con: Bypassable by root (`CAP_DAC_OVERRIDE`).
- Con: Returns `EACCES`, not `EROFS` — a different error from namespace mode.
- Con: Creates a false impression of the same guarantee provided by bind-remount-ro.
