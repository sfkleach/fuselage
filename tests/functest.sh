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

# True if a C compiler the bundler can use is available. fuselage-bundle prefers
# musl-gcc and falls back to gcc, so either is sufficient to build the stub.
have_cc() {
    command -v gcc >/dev/null 2>&1 || command -v musl-gcc >/dev/null 2>&1
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

# ── Test group: --extract ─────────────────────────────────────────────────────

echo "--- --extract ---"

# --extract=force skips the namespace entirely; dynamic content is still readable.
check_output "--extract=force dynamic content readable" "hello from archive" \
    "$FUSELAGE" --extract=force --dynamic="$WORKDIR/data.zip" \
        -- sh -c 'cat "$FUSELAGE_DYNAMIC/data/hello.txt"'

# --extract=force with --static: content is readable (not read-only enforced, but accessible).
check_output "--extract=force static content readable" "hello from archive" \
    "$FUSELAGE" --extract=force --static="$WORKDIR/data.zip" \
        -- sh -c 'cat "$FUSELAGE_STATIC/data/hello.txt"'

# --extract=force rejects fixed-path mounts at parse time.
check_fails "--extract=force rejects fixed-path mount" \
    "$FUSELAGE" --extract=force --dynamic="/run/fuselage/myapp:$WORKDIR/data.zip" -- true

# --extract=allow rejects fixed-path mounts at parse time.
check_fails "--extract=allow rejects fixed-path mount" \
    "$FUSELAGE" --extract=allow --dynamic="/run/fuselage/myapp:$WORKDIR/data.zip" -- true

# --extract=deny (explicit default) behaves identically to omitting the flag.
check_output "--extract=deny behaves as default" "hello from archive" \
    "$FUSELAGE" --extract=deny --dynamic="$WORKDIR/data.zip" \
        -- sh -c 'cat "$FUSELAGE_DYNAMIC/data/hello.txt"'

# --extract=prefer uses extract mode when unprivileged (no user namespace),
# so dynamic content is accessible and static is NOT read-only.
check_output "--extract=prefer dynamic content readable" "hello from archive" \
    "$FUSELAGE" --extract=prefer --dynamic="$WORKDIR/data.zip" \
        -- sh -c 'cat "$FUSELAGE_DYNAMIC/data/hello.txt"'

if [[ "$MODE" == "setuid" ]]; then
    # In setuid mode --extract=prefer uses a plain mount namespace (no UID
    # remapping), so static archives ARE read-only — same as normal mode.
    check "--extract=prefer static is read-only in setuid mode" \
        "$FUSELAGE" --extract=prefer --static="$WORKDIR/data.zip" -- \
            sh -c '! touch "$FUSELAGE_STATIC/data/probe" 2>/dev/null'
fi

# Verify that the procdir is fully removed on exit even when extracted content
# includes read-only directories (mode 0555) and files (mode 0444).  Capture
# the procdir path during the run, then assert it no longer exists afterwards.
_procdir=$(
    "$FUSELAGE" --extract=force --static="$WORKDIR/rodir.zip" \
        -- sh -c 'dirname "$FUSELAGE_TMPDIR"'
)
check "--extract=force cleans up read-only extracted content on exit" \
    test -n "$_procdir" -a ! -e "$_procdir"

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

# ── Test group: base64 archives ──────────────────────────────────────────────

echo "--- base64 archives ---"

# Build a base64-encoded zip fixture: a single executable script.
python3 - <<EOF
import zipfile, base64
z = zipfile.ZipFile("$WORKDIR/b64src.zip", "w")
info = zipfile.ZipInfo("bin/hello.sh")
info.external_attr = (0o755 << 16)
z.writestr(info, '#!/bin/sh\necho "hello from base64"\n')
z.close()
with open("$WORKDIR/b64src.zip", "rb") as f:
    data = base64.b64encode(f.read()).decode()
with open("$WORKDIR/plain.b64", "w") as f:
    f.write(data + "\n")
EOF

# Build the same fixture as a valid herescript shebang script.
python3 - <<EOF
import zipfile, base64, textwrap
with open("$WORKDIR/b64src.zip", "rb") as f:
    data = base64.b64encode(f.read()).decode()
with open("$WORKDIR/commented.b64", "w") as f:
    f.write("#!/usr/bin/herescript \$FUSELAGE\n")
    f.write("#: --static b64src:\${HERESCRIPT_FILE}\n")
    f.write("#: --run b64src/bin/hello.sh\n")
    for chunk in textwrap.wrap(data, 76):
        f.write(chunk + "\n")
EOF

# --dynamic accepts a plain base64-encoded zip.
check_output "dynamic base64 zip content visible" "hello from base64" \
    "$FUSELAGE" --dynamic="b64src:$WORKDIR/plain.b64" -- \
        sh -c 'sh "$FUSELAGE_DYNAMIC/b64src/bin/hello.sh"'

# --dynamic accepts a base64 zip with leading '#' comment lines.
check_output "dynamic base64 zip with comments" "hello from base64" \
    "$FUSELAGE" --dynamic="b64src:$WORKDIR/commented.b64" -- \
        sh -c 'sh "$FUSELAGE_DYNAMIC/b64src/bin/hello.sh"'

# --static accepts a plain base64-encoded zip.
check_output "static base64 zip content visible" "hello from base64" \
    "$FUSELAGE" --static="b64src:$WORKDIR/plain.b64" -- \
        sh -c 'sh "$FUSELAGE_STATIC/b64src/bin/hello.sh"'

# --run works with a base64 archive.
check_output "--run with base64 archive" "hello from base64" \
    "$FUSELAGE" --dynamic="b64src:$WORKDIR/plain.b64" --run b64src/bin/hello.sh

# Build a base64-encoded squashfs fixture if mksquashfs is available.
if command -v mksquashfs >/dev/null 2>&1; then
    mkdir -p "$WORKDIR/sfsroot/bin"
    printf '#!/bin/sh\necho "hello from b64 squashfs"\n' > "$WORKDIR/sfsroot/bin/hello.sh"
    chmod +x "$WORKDIR/sfsroot/bin/hello.sh"
    mksquashfs "$WORKDIR/sfsroot" "$WORKDIR/b64.sfs" -comp zstd -noappend -quiet
    python3 - <<EOF
import base64
with open("$WORKDIR/b64.sfs", "rb") as f:
    data = base64.b64encode(f.read()).decode()
with open("$WORKDIR/squashfs.b64", "w") as f:
    f.write("# squashfs image\n")
    f.write(data + "\n")
EOF
    check_output "dynamic base64 squashfs content visible" "hello from b64 squashfs" \
        "$FUSELAGE" --dynamic="sfs:$WORKDIR/squashfs.b64" -- \
            sh -c 'sh "$FUSELAGE_DYNAMIC/sfs/bin/hello.sh"'
else
    echo "  SKIP: base64 squashfs (mksquashfs not installed)"
fi

echo ""

# ── fuselage-bundle round-trip ─────────────────────────────────────────────────
# Pack a squashfs into a self-executing ELF, then verify both that fuselage can
# mount the embedded squashfs and that the bundle runs itself end to end.

echo "--- fuselage-bundle ---"

BUNDLE="$(dirname "$FUSELAGE")/fuselage-bundle"

if command -v mksquashfs >/dev/null 2>&1 && have_cc && [[ -x "$BUNDLE" ]]; then
    mkdir -p "$WORKDIR/bundleroot"
    printf '#!/bin/sh\necho "hello from bundled squashfs"\n' > "$WORKDIR/bundleroot/run.sh"
    chmod +x "$WORKDIR/bundleroot/run.sh"
    mksquashfs "$WORKDIR/bundleroot" "$WORKDIR/bundle.sfs" -comp zstd -noappend -quiet

    # Pack: the baked --static points at /proc/self/exe so the bundle mounts its
    # own embedded squashfs; the stub substitutes the resolved path at runtime.
    "$BUNDLE" --archive="$WORKDIR/bundle.sfs" --output="$WORKDIR/selfexec" \
        -- --static=payload:/proc/self/exe --run payload/run.sh >/dev/null

    # The bundle must be a recognisable ELF that fuselage can mount directly,
    # proving the embedded-squashfs offset agrees between packer and detector.
    check_output "bundle: fuselage mounts embedded squashfs" "hello from bundled squashfs" \
        "$FUSELAGE" --static="payload:$WORKDIR/selfexec" --run payload/run.sh

    # Run the self-executing binary directly. The stub execs "fuselage" via PATH,
    # so force the directory of the fuselage-under-test to the front of PATH —
    # otherwise an installed fuselage could be exercised instead of this build.
    FUSELAGE_DIR="$(cd "$(dirname "$FUSELAGE")" && pwd)"
    check_output "bundle: self-executing binary runs" "hello from bundled squashfs" \
        env "PATH=$FUSELAGE_DIR:$PATH" "$WORKDIR/selfexec"
else
    echo "  SKIP: fuselage-bundle (requires mksquashfs, gcc or musl-gcc, and a built fuselage-bundle)"
fi

# ── fuselage-bundle argument validation ────────────────────────────────────────
# These tests do not require mksquashfs or gcc; they exercise early-exit checks.

if [[ -x "$BUNDLE" ]]; then
    BDIR="$WORKDIR/bundle-argtest-dir"
    rm -rf "$BDIR"

    # --output inside --build-dir without --keep must be rejected before any work.
    check_fails "bundle: --output inside --build-dir is rejected" \
        "$BUNDLE" --archive="$WORKDIR/data.zip" \
                  --build-dir="$BDIR" \
                  --output="$BDIR/out"

    # The build-dir must not have been created: the check fires before mkdir.
    if [[ ! -d "$BDIR" ]]; then
        pass "bundle: build-dir not created on early rejection"
    else
        fail "bundle: build-dir not created on early rejection (directory exists)"
        rm -rf "$BDIR"
    fi
else
    echo "  SKIP: fuselage-bundle argument validation (fuselage-bundle not built)"
fi

echo ""

# ── uv-bundle round-trip ──────────────────────────────────────────────────────
# Run the full uv-bundle pipeline — fuselage dynamic-empty, uv sync, mksquashfs,
# fuselage-bundle — then execute the resulting self-extracting binary to verify
# each stage completed correctly.
#
# The entry point is the stdlib 'platform' module so no Python source needs to
# be installed into the venv; the test works with a bare project definition.

echo "--- uv-bundle ---"

UV_BUNDLE="$(dirname "$FUSELAGE")/uv-bundle"

if command -v uv >/dev/null 2>&1 && command -v mksquashfs >/dev/null 2>&1 && \
        have_cc && [[ -x "$BUNDLE" ]] && [[ -x "$UV_BUNDLE" ]] && \
        [[ "$MODE" == "setuid" ]]; then

    PROJDIR="$WORKDIR/uvbundle-project"
    mkdir -p "$PROJDIR"

    # Minimal project with no Python source.  'uv sync --no-install-project'
    # creates the venv without trying to build or install the project package,
    # which avoids the need for a build backend and keeps the test self-contained.
    cat > "$PROJDIR/pyproject.toml" <<'PYPROJ'
[project]
name = "uvbundleapp"
version = "0.1.0"
requires-python = ">=3.8"
PYPROJ

    FUSELAGE_DIR="$(cd "$(dirname "$FUSELAGE")" && pwd)"

    # Build the bundle.  --module=platform targets the Python stdlib so the
    # venv needs no installed user package.  --uv-arg=--no-install-project
    # tells uv not to attempt building or installing this project.
    # PATH is extended so uv-bundle's internal fuselage and fuselage-bundle
    # calls resolve to the binaries under test rather than any installed versions.
    if env "PATH=$FUSELAGE_DIR:$PATH" "$UV_BUNDLE" \
            --project="$PROJDIR" \
            --module=platform \
            --uv-arg=--no-install-project \
            --output="$WORKDIR/uvbundle-out" \
            >/dev/null 2>&1; then
        pass "uv-bundle: pipeline completed without error"

        check "uv-bundle: output binary is executable" \
            test -x "$WORKDIR/uvbundle-out"

        # Run the self-executing binary end-to-end.  Prepend the test build
        # directory to PATH so the bundle's stub invokes our fuselage under test
        # rather than any version that may be installed system-wide.
        check "uv-bundle: self-executing binary runs python -m platform" \
            env "PATH=$FUSELAGE_DIR:$PATH" "$WORKDIR/uvbundle-out"

        # ── requires-python version check ─────────────────────────────────────
        # A project demanding an impossible Python version must be rejected by
        # uv-bundle's own check before any build work.  '>=99.0' cannot be
        # satisfied by any system Python, so this is stable across machines.
        BADPROJ="$WORKDIR/uvbundle-badpython"
        mkdir -p "$BADPROJ"
        cat > "$BADPROJ/pyproject.toml" <<'PYPROJ'
[project]
name = "uvbundlebadpy"
version = "0.1.0"
requires-python = ">=99.0"
PYPROJ

        # Without --ignore-version-mismatch the build must fail, and the error
        # must come from our version check (not a later uv sync failure).
        badpy_status=0
        badpy_out="$(env "PATH=$FUSELAGE_DIR:$PATH" "$UV_BUNDLE" \
            --project="$BADPROJ" \
            --module=platform \
            --uv-arg=--no-install-project \
            --output="$WORKDIR/uvbundle-badpy-out" 2>&1)" || badpy_status=$?
        if [[ $badpy_status -ne 0 ]] \
            && grep -q "Install a compatible Python, or pass --ignore-version-mismatch to build anyway." <<<"$badpy_out" \
            && ! grep -q "continuing because --ignore-version-mismatch" <<<"$badpy_out"; then
            pass "uv-bundle: incompatible requires-python is rejected by the version check"
        else
            fail "uv-bundle: incompatible requires-python not rejected as expected (got: $badpy_out)"
        fi

        # With --ignore-version-mismatch the version check is downgraded to a
        # warning, so the build proceeds past it (and then fails later at uv
        # sync, which cannot find Python 99 — that is expected and not our
        # concern here).  Assert the warning appears and the hard check error
        # does not.
        ignore_out="$(env "PATH=$FUSELAGE_DIR:$PATH" "$UV_BUNDLE" \
            --project="$BADPROJ" \
            --module=platform \
            --uv-arg=--no-install-project \
            --ignore-version-mismatch \
            --output="$WORKDIR/uvbundle-ignore-out" 2>&1 || true)"
        if grep -q "continuing because --ignore-version-mismatch" <<<"$ignore_out" \
            && ! grep -q "Install a compatible Python, or pass --ignore-version-mismatch to build anyway." <<<"$ignore_out"; then
            pass "uv-bundle: --ignore-version-mismatch downgrades the check to a warning"
        else
            fail "uv-bundle: --ignore-version-mismatch did not downgrade the check (got: $ignore_out)"
        fi

        # ── [project.scripts] entry point (--script) ──────────────────────────
        # Bundle the tests/hello fixture targeting its console script rather than
        # a module.  The fixture is copied into the work dir first so the build
        # (uv sync, .venv, *.egg-info) does not write artefacts into the repo.
        HELLO_SRC="$(dirname "$0")/hello"
        if [[ -f "$HELLO_SRC/pyproject.toml" ]]; then
            rm -rf "$WORKDIR/uvbundle-hello"
            cp -r "$HELLO_SRC" "$WORKDIR/uvbundle-hello"

            if env "PATH=$FUSELAGE_DIR:$PATH" "$UV_BUNDLE" \
                    --project="$WORKDIR/uvbundle-hello" \
                    --script=hello \
                    --output="$WORKDIR/uvbundle-script-out" \
                    >/dev/null 2>&1; then
                # The console script prints a fixed greeting; check it runs.
                check_output "uv-bundle: --script runs the [project.scripts] entry point" \
                    "Hello, World!" \
                    env "PATH=$FUSELAGE_DIR:$PATH" "$WORKDIR/uvbundle-script-out"
            else
                fail "uv-bundle: --script failed to build bundle"
            fi

            # An undeclared script name must be rejected before any build work.
            script_err="$(env "PATH=$FUSELAGE_DIR:$PATH" "$UV_BUNDLE" \
                --project="$WORKDIR/uvbundle-hello" \
                --script=does-not-exist \
                --output="$WORKDIR/uvbundle-script-bad" 2>&1 || true)"
            if grep -q "is not in \[project.scripts\]" <<<"$script_err"; then
                pass "uv-bundle: unknown --script is rejected with available list"
            else
                fail "uv-bundle: unknown --script not rejected as expected (got: $script_err)"
            fi
        else
            echo "  SKIP: uv-bundle --script (tests/hello fixture not found)"
        fi
    else
        fail "uv-bundle: pipeline failed to build bundle"
    fi
else
    echo "  SKIP: uv-bundle (requires setuid mode, uv, mksquashfs, gcc or musl-gcc, fuselage-bundle, and uv-bundle)"
fi

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
