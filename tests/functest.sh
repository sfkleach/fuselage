#!/usr/bin/env bash
# Functional tests for fuselage.
#
# Usage: functest.sh <binary> <mode>
#   binary  path to the fuselage binary to test
#   mode    "plain" or "setuid"
#
# Exit code: 0 if all tests passed, 1 if any failed.

set -euo pipefail

FUSELAGE="${1:?usage: functest.sh <binary> <mode>}"
MODE="${2:?usage: functest.sh <binary> <mode>}"

PASS=0
FAIL=0

# ── Helpers ──────────────────────────────────────────────────────────────────

pass() { echo "  PASS: $1"; (( PASS++ )) || true; }
fail() { echo "  FAIL: $1"; (( FAIL++ )) || true; }

check() {
    local name="$1"; shift
    if "$@" >/dev/null 2>&1; then
        pass "$name"
    else
        fail "$name"
    fi
}

check_output() {
    local name="$1"
    local expected="$2"; shift 2
    local actual
    actual=$("$@" 2>/dev/null) || true
    if [[ "$actual" == *"$expected"* ]]; then
        pass "$name"
    else
        fail "$name (expected '$expected', got '$actual')"
    fi
}

check_fails() {
    local name="$1"; shift
    if "$@" >/dev/null 2>&1; then
        fail "$name (expected failure but succeeded)"
    else
        pass "$name"
    fi
}

# ── Fixture setup ─────────────────────────────────────────────────────────────

WORKDIR="$(dirname "$0")/../_build/functest-fixtures"
mkdir -p "$WORKDIR"
trap 'rm -rf "$(dirname "$0")/../_build/functest-fixtures"' EXIT

# Create a small zip containing a text file.
# Use ZipInfo with explicit external_attr so files get mode 0644 on extraction.
# writestr(name, data) leaves external_attr=0, which produces mode 0000 on
# Linux — unreadable by non-root users (breaks setuid mode tests).
python3 - <<EOF
import zipfile
def zinfo(name, mode=0o644):
    zi = zipfile.ZipInfo(name)
    zi.external_attr = (mode << 16)
    return zi
z = zipfile.ZipFile("$WORKDIR/data.zip", "w")
z.writestr(zinfo("hello.txt"), "hello from archive\n")
z.writestr(zinfo("subdir/nested.txt"), "nested content\n")
z.close()
EOF

# Create a zip containing a small executable script.
python3 - <<EOF
import zipfile, stat
z = zipfile.ZipFile("$WORKDIR/app.zip", "w")
info = zipfile.ZipInfo("bin/greet")
info.external_attr = (0o755 << 16)   # rwxr-xr-x
z.writestr(info, '#!/bin/sh\necho "hello \$@"\n')
z.close()
EOF

# Create a zip with a read-only directory (mode 0555) to test reaper resilience.
python3 - <<EOF
import zipfile, stat
z = zipfile.ZipFile("$WORKDIR/rodir.zip", "w")
di = zipfile.ZipInfo("locked/")
di.external_attr = (0o555 << 16)
z.writestr(di, "")
fi = zipfile.ZipInfo("locked/file.txt")
fi.external_attr = (0o444 << 16)
z.writestr(fi, "protected\n")
z.close()
EOF

echo "=== fuselage functional tests ($MODE mode: $FUSELAGE) ==="
echo ""

# ── Test group: basic environment ─────────────────────────────────────────────

echo "--- basic environment ---"

# FUSELAGE_TMPDIR is set and is a directory.
check_output "FUSELAGE_TMPDIR is set" "/" \
    "$FUSELAGE" -- sh -c 'test -d "$FUSELAGE_TMPDIR" && echo "/"'

# FUSELAGE_TMPDIR is writable.
check "FUSELAGE_TMPDIR is writable" \
    "$FUSELAGE" -- sh -c 'touch "$FUSELAGE_TMPDIR/probe" && rm "$FUSELAGE_TMPDIR/probe"'

# FUSELAGE_DYNAMIC is NOT set when no --dynamic given.
check "FUSELAGE_DYNAMIC unset without --dynamic" \
    "$FUSELAGE" -- sh -c '[ -z "${FUSELAGE_DYNAMIC-}" ]'

# FUSELAGE_STATIC is NOT set when no --static given.
check "FUSELAGE_STATIC unset without --static" \
    "$FUSELAGE" -- sh -c '[ -z "${FUSELAGE_STATIC-}" ]'

# Child exit code is propagated.
"$FUSELAGE" -- sh -c 'exit 42' > /dev/null 2>&1 || EXIT_CODE=$?
if [[ "${EXIT_CODE:-0}" -eq 42 ]]; then
    pass "exit code propagated"
else
    fail "exit code propagated (got ${EXIT_CODE:-0}, expected 42)"
fi
unset EXIT_CODE

echo ""

# ── Test group: --dynamic ─────────────────────────────────────────────────────

echo "--- --dynamic ---"

# FUSELAGE_DYNAMIC is set when --dynamic is given.
check "FUSELAGE_DYNAMIC is set" \
    "$FUSELAGE" --dynamic="$WORKDIR/data.zip" -- sh -c '[ -n "$FUSELAGE_DYNAMIC" ]'

# Archive content is visible.
check_output "dynamic content visible" "hello from archive" \
    "$FUSELAGE" --dynamic="$WORKDIR/data.zip" -- \
        sh -c 'cat "$FUSELAGE_DYNAMIC/data/hello.txt"'

# Nested content visible.
check_output "dynamic nested content visible" "nested content" \
    "$FUSELAGE" --dynamic="$WORKDIR/data.zip" -- \
        sh -c 'cat "$FUSELAGE_DYNAMIC/data/subdir/nested.txt"'

# Dynamic archive is writable.
check "dynamic archive is writable" \
    "$FUSELAGE" --dynamic="$WORKDIR/data.zip" -- \
        sh -c 'touch "$FUSELAGE_DYNAMIC/data/newfile"'

# NAME: prefix overrides derived name.
check_output "dynamic NAME: prefix" "hello from archive" \
    "$FUSELAGE" --dynamic="mydata:$WORKDIR/data.zip" -- \
        sh -c 'cat "$FUSELAGE_DYNAMIC/mydata/hello.txt"'

# -d short form works.
check "dynamic -d short form" \
    "$FUSELAGE" -d "$WORKDIR/data.zip" -- sh -c '[ -d "$FUSELAGE_DYNAMIC/data" ]'

# Multiple --dynamic archives.
check "multiple dynamic archives" \
    "$FUSELAGE" --dynamic="a:$WORKDIR/data.zip" --dynamic="b:$WORKDIR/app.zip" -- \
        sh -c '[ -d "$FUSELAGE_DYNAMIC/a" ] && [ -d "$FUSELAGE_DYNAMIC/b" ]'

# Duplicate archive name is rejected.
check_fails "duplicate dynamic names rejected" \
    "$FUSELAGE" --dynamic="$WORKDIR/data.zip" --dynamic="$WORKDIR/data.zip" -- true

echo ""

# ── Test group: --static ──────────────────────────────────────────────────────

echo "--- --static ---"

# FUSELAGE_STATIC is set when --static is given.
check "FUSELAGE_STATIC is set" \
    "$FUSELAGE" --static="$WORKDIR/data.zip" -- sh -c '[ -n "$FUSELAGE_STATIC" ]'

# Static content is visible.
check_output "static content visible" "hello from archive" \
    "$FUSELAGE" --static="$WORKDIR/data.zip" -- \
        sh -c 'cat "$FUSELAGE_STATIC/data/hello.txt"'

# Static archive is read-only.
check "static archive is read-only" \
    "$FUSELAGE" --static="$WORKDIR/data.zip" -- \
        sh -c '! touch "$FUSELAGE_STATIC/data/probe" 2>/dev/null'

# Compute the cache key for data.zip (first 16 hex chars of sha256).
# This matches the logic in archive::compute_sha256.
DATA_HASH=$(sha256sum "$WORKDIR/data.zip" | cut -c1-16)
CACHE_DIR="$HOME/.fuselage/cache"
# Remove any pre-existing cache entry for this fixture so the miss test is reliable.
rm -f "$CACHE_DIR/$DATA_HASH.sfs" "$CACHE_DIR/$DATA_HASH.complete"

# No cache written without --cache-static.
"$FUSELAGE" --static="$WORKDIR/data.zip" -- true >/dev/null 2>&1 || true
if [[ ! -f "$CACHE_DIR/$DATA_HASH.complete" ]]; then
    pass "no cache written without --cache-static"
else
    fail "no cache written without --cache-static (sentinel appeared)"
fi

# --cache-static writes a cache entry (cache miss path).
"$FUSELAGE" --cache-static --static="$WORKDIR/data.zip" -- true >/dev/null 2>&1 || true
if [[ -f "$CACHE_DIR/$DATA_HASH.complete" ]]; then
    pass "--cache-static writes cache entry"
else
    fail "--cache-static writes cache entry (sentinel missing)"
fi

# Cache hit: second run with same archive and --cache-static succeeds.
check "--cache-static cache hit succeeds" \
    "$FUSELAGE" --cache-static --static="$WORKDIR/data.zip" -- \
        sh -c 'cat "$FUSELAGE_STATIC/data/hello.txt"'

# NAME: prefix for --static.
check_output "static NAME: prefix" "hello from archive" \
    "$FUSELAGE" --static="mydata:$WORKDIR/data.zip" -- \
        sh -c 'cat "$FUSELAGE_STATIC/mydata/hello.txt"'

# -s short form works.
check "static -s short form" \
    "$FUSELAGE" -s "$WORKDIR/data.zip" -- sh -c '[ -d "$FUSELAGE_STATIC/data" ]'

# Cross-flag duplicate name rejected.
check_fails "cross-flag duplicate names rejected" \
    "$FUSELAGE" --dynamic="data:$WORKDIR/data.zip" --static="data:$WORKDIR/data.zip" -- true

echo ""

# ── Test group: --run ─────────────────────────────────────────────────────────

echo "--- --run ---"

# --run finds and executes an executable in a dynamic archive.
check_output "--run executes dynamic executable" "hello world" \
    "$FUSELAGE" --dynamic="$WORKDIR/app.zip" --run app/bin/greet world

# --run passes extra arguments.
check_output "--run passes extra arguments" "hello foo bar" \
    "$FUSELAGE" --dynamic="$WORKDIR/app.zip" --run app/bin/greet foo bar

# --run rejects an absolute path.
check_fails "--run rejects absolute path" \
    "$FUSELAGE" --dynamic="$WORKDIR/app.zip" --run /app/bin/greet

# --run rejects a path whose first component names no archive.
check_fails "--run rejects unknown archive name" \
    "$FUSELAGE" --dynamic="$WORKDIR/app.zip" --run nosucharchive/bin/greet

# --run without any archive is rejected.
check_fails "--run without archive rejected" \
    "$FUSELAGE" --run app/bin/greet

echo ""

# ── Test group: error cases ───────────────────────────────────────────────────

echo "--- error cases ---"

# No command and no --run is rejected.
check_fails "no command rejected" "$FUSELAGE"

# Non-existent archive file is rejected.
check_fails "missing archive file rejected" \
    "$FUSELAGE" --dynamic="$WORKDIR/nosuch.zip" -- true

# A file whose stem is empty (e.g. ".zip") must be rejected with a clear error.
python3 -c "
import zipfile, os
z = zipfile.ZipFile('$WORKDIR/.zip', 'w')
z.writestr('x.txt', 'x')
z.close()
"
check_fails "empty derived name rejected" \
    "$FUSELAGE" --dynamic="$WORKDIR/.zip" -- true

echo ""

# ── Setuid-specific tests ─────────────────────────────────────────────────────

if [[ "$MODE" == "setuid" ]]; then
    echo "--- setuid-specific ---"

    REAL_USER=$(id -un)

    # Inside the namespace the process sees its real UID (not root).
    check_output "real uid preserved in namespace" "$REAL_USER" \
        "$FUSELAGE" -- whoami

    # In setuid mode fuselage uses a plain mount namespace (not a user namespace).
    # The uid_map for a plain mount namespace is the identity mapping over the
    # full uid range: "0 0 4294967295". In a user namespace it would be a
    # narrow mapping such as "0 1000 1".
    check "no user namespace in setuid mode" \
        "$FUSELAGE" -- sh -c "grep -qE '^[[:space:]]*0[[:space:]]+0[[:space:]]+4294967295' /proc/self/uid_map"

    echo ""
fi

# ── Summary ───────────────────────────────────────────────────────────────────

TOTAL=$(( PASS + FAIL ))
echo "Results: $PASS/$TOTAL passed"
if [[ $FAIL -gt 0 ]]; then
    echo "FAILED: $FAIL test(s) failed"
    exit 1
fi
