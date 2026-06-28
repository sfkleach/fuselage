#!/usr/bin/env bash
# Container-based functional tests for fuselage --extract modes.
#
# Verifies that --extract=force and --extract=allow work inside a locked-down
# container where unshare(2) is blocked by a seccomp filter.  This simulates
# the real-world scenario that --extract was designed to handle: environments
# where unprivileged user namespaces are disabled or unshare is blocked.
#
# Usage: conttest.sh <binary>
#   binary  path to the fuselage binary to test
#
# Optional environment variable:
#   FUSELAGE_CONT_IMAGE  container image to use (default: ubuntu:22.04)
#                        The image must be available locally.
#                        Run: podman pull ubuntu:22.04
#
# Requires: podman
# Exit code: 0 if all tests passed, 1 if any failed.

set -euo pipefail

FUSELAGE="$(realpath "${1:?usage: conttest.sh <binary>}")"
IMAGE="${FUSELAGE_CONT_IMAGE:-ubuntu:22.04}"

# ── Prerequisite checks ───────────────────────────────────────────────────────

if ! command -v podman >/dev/null 2>&1; then
    echo "SKIP: conttest.sh (podman not installed)"
    exit 0
fi

if ! podman image exists "$IMAGE" 2>/dev/null; then
    echo "SKIP: conttest.sh (image '$IMAGE' not available locally)"
    echo "      Run: podman pull $IMAGE"
    exit 0
fi

# ── Helpers ───────────────────────────────────────────────────────────────────

PASS=0
FAIL=0

pass() { echo "  PASS: $1"; (( PASS++ )) || true; }
fail() { echo "  FAIL: $1"; (( FAIL++ )) || true; }

# run_locked <fuselage-args...>: run fuselage in the locked container.
# The container has unshare(2) blocked by a seccomp filter, the fuselage
# binary mounted at /usr/local/bin/fuselage, and fixtures at /fixtures.
run_locked() {
    podman run --rm \
        --security-opt "seccomp=$WORKDIR/no-unshare.json" \
        -v "$FUSELAGE:/usr/local/bin/fuselage:ro" \
        -v "$WORKDIR:/fixtures:ro" \
        "$IMAGE" \
        /usr/local/bin/fuselage "$@"
}

# run_locked_sh <script>: run an arbitrary shell snippet in the locked container.
run_locked_sh() {
    podman run --rm \
        --security-opt "seccomp=$WORKDIR/no-unshare.json" \
        -v "$FUSELAGE:/usr/local/bin/fuselage:ro" \
        -v "$WORKDIR:/fixtures:ro" \
        "$IMAGE" \
        sh -c "$1"
}

check() {
    local name="$1"; shift
    if run_locked "$@" >/dev/null 2>&1; then
        pass "$name"
    else
        fail "$name"
    fi
}

check_output() {
    local name="$1"
    local expected="$2"; shift 2
    local actual
    actual=$(run_locked "$@" 2>/dev/null) || true
    if [[ "$actual" == *"$expected"* ]]; then
        pass "$name"
    else
        fail "$name (expected '$expected', got '$actual')"
    fi
}

check_fails() {
    local name="$1"; shift
    if run_locked "$@" >/dev/null 2>&1; then
        fail "$name (expected failure but succeeded)"
    else
        pass "$name"
    fi
}

# check_stderr: assert that the given string appears somewhere in stderr output.
# The command is expected to exit with any code; only stderr content is checked.
check_stderr() {
    local name="$1"
    local expected="$2"; shift 2
    local actual
    # Capture stderr only: 2>&1 redirects stderr to the capture pipe, then
    # >/dev/null discards stdout so it does not appear in the captured output.
    actual=$(run_locked "$@" 2>&1 >/dev/null) || true
    if [[ "$actual" == *"$expected"* ]]; then
        pass "$name"
    else
        fail "$name (expected '$expected' in stderr, got '$actual')"
    fi
}

# ── Fixture setup ─────────────────────────────────────────────────────────────

WORKDIR="$(cd "$(dirname "$0")/.." && pwd)/_build/conttest-fixtures"
mkdir -p "$WORKDIR"
trap 'rm -rf "$(dirname "$0")/../_build/conttest-fixtures"' EXIT

# Seccomp profile that blocks unshare(2) with EPERM, simulating a container
# environment where unprivileged user namespaces have been disabled.
cat > "$WORKDIR/no-unshare.json" << 'SECCOMPEOF'
{
  "defaultAction": "SCMP_ACT_ALLOW",
  "syscalls": [
    {
      "names": ["unshare"],
      "action": "SCMP_ACT_ERRNO",
      "errnoRet": 1
    }
  ]
}
SECCOMPEOF

# Small zip with a text file.
python3 - <<EOF
import zipfile
def zinfo(name, mode=0o644):
    zi = zipfile.ZipInfo(name)
    zi.external_attr = (mode << 16)
    return zi
z = zipfile.ZipFile("$WORKDIR/data.zip", "w")
z.writestr(zinfo("hello.txt"), "hello from archive\n")
z.close()
EOF

# Zip with a read-only directory (mode 0555) to exercise cleanup resilience.
python3 - <<EOF
import zipfile
z = zipfile.ZipFile("$WORKDIR/rodir.zip", "w")
di = zipfile.ZipInfo("locked/")
di.external_attr = (0o555 << 16)
z.writestr(di, "")
fi = zipfile.ZipInfo("locked/file.txt")
fi.external_attr = (0o444 << 16)
z.writestr(fi, "protected\n")
z.close()
EOF

echo "=== fuselage container tests (image: $IMAGE) ==="
echo ""

# ── Test group: locked container (unshare blocked) ────────────────────────────

echo "--- locked container: unshare blocked ---"

# Baseline: fuselage without --extract fails because unshare is blocked.
check_fails "default (no --extract) fails when unshare blocked" \
    --dynamic="/fixtures/data.zip" -- sh -c 'true'

# --extract=deny (explicit default) also fails when unshare is blocked.
check_fails "--extract=deny fails when unshare blocked" \
    --extract=deny --dynamic="/fixtures/data.zip" -- sh -c 'true'

# --extract=force always skips the namespace; dynamic content is accessible.
check_output "--extract=force dynamic content readable" "hello from archive" \
    --extract=force --dynamic="/fixtures/data.zip" \
    -- sh -c 'cat "$FUSELAGE_DYNAMIC/data/hello.txt"'

# --extract=force with --static: content is accessible (not read-only enforced).
check_output "--extract=force static content readable" "hello from archive" \
    --extract=force --static="/fixtures/data.zip" \
    -- sh -c 'cat "$FUSELAGE_STATIC/data/hello.txt"'

# --extract=allow falls back to extract-and-run when unshare is blocked.
check_output "--extract=allow falls back and succeeds" "hello from archive" \
    --extract=allow --dynamic="/fixtures/data.zip" \
    -- sh -c 'cat "$FUSELAGE_DYNAMIC/data/hello.txt"'

# --extract=allow must emit a warning to stderr when the fallback is taken.
check_stderr "--extract=allow emits fallback warning to stderr" "extract-and-run" \
    --extract=allow --dynamic="/fixtures/data.zip" -- sh -c 'true'

# --extract=prefer fails in a locked container when running as root: prefer uses
# namespace mode for root (no UID remapping to avoid), so it hits the blocked
# unshare just like --extract=deny.  Use --extract=allow or --extract=force
# when namespace creation may be blocked.
check_fails "--extract=prefer fails as root when unshare blocked" \
    --extract=prefer --dynamic="/fixtures/data.zip" -- sh -c 'true'

# Fixed-path mounts are rejected at parse time regardless of the lock status.
check_fails "--extract=force rejects fixed-path mount" \
    --extract=force --dynamic="/run/fuselage/myapp:/fixtures/data.zip" -- true

# Verify that the procdir is fully removed on exit even when read-only
# directories (mode 0555) and files (mode 0444) were extracted.  Both the
# fuselage invocation and the post-exit check must run inside the same
# container so that the procdir path is verifiable after fuselage exits.
if run_locked_sh '
    _pd=$(/usr/local/bin/fuselage --extract=force --static=/fixtures/rodir.zip \
        -- sh -c "dirname \"\$FUSELAGE_TMPDIR\"")
    test -n "$_pd" && test ! -e "$_pd"
' >/dev/null 2>&1; then
    pass "--extract=force cleans up read-only extracted content on exit"
else
    fail "--extract=force cleans up read-only extracted content on exit"
fi

echo ""

# ── Summary ───────────────────────────────────────────────────────────────────

TOTAL=$(( PASS + FAIL ))
echo "Results: $PASS/$TOTAL passed"
if [[ $FAIL -gt 0 ]]; then
    echo "FAILED: $FAIL test(s) failed"
    exit 1
fi
