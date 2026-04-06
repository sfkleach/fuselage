default:
    @just --list

shippable:
    python3 scripts/check-changelog.py

test: unittest lint fmt-check audit functest

unittest:
    cargo test

lint:
    cargo clippy --all-targets -- -D warnings

audit:
    @echo "Running security audit..."
    @if command -v cargo-audit >/dev/null 2>&1; then \
        cargo audit; \
    else \
        echo "cargo-audit not found, skipping security audit"; \
        echo "To install cargo-audit, run: just install-audit"; \
    fi

# Install cargo-audit
install-audit:
    cargo install cargo-audit

fmt:
    cargo fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt --check

build-debug:
    cargo build
    ls -alh target/debug/fuselage

build-release:
    cargo build --release
    ls -alh target/release/fuselage

clear-executable-debug:
    rm -f target/debug/fuselage

clear-executable-release:
    rm -f target/release/fuselage

clear-executables: clear-executable-debug clear-executable-release

functest-debug:
    #!/usr/bin/env bash
    set -euo pipefail
    rm -f target/debug/fuselage
    cargo build
    bash tests/functest.sh target/debug/fuselage plain

functest-debug-run:
    bash tests/functest.sh target/debug/fuselage plain

functest-setuid-run:
    bash tests/functest.sh target/release/fuselage setuid

functest-setuid:
    #!/usr/bin/env bash
    set -euo pipefail
    rm -f target/release/fuselage
    cargo build --release
    if [ "$(id -u)" = "0" ]; then
        chown root:root target/release/fuselage
        chmod u+s target/release/fuselage
    else
        sudo chown root:root target/release/fuselage
        sudo chmod u+s target/release/fuselage
    fi
    bash tests/functest.sh target/release/fuselage setuid

functest: functest-debug functest-setuid


setuid-debug:
    #!/usr/bin/env bash
    if [ "$(id -u)" = "0" ]; then
        chown root:root target/debug/fuselage
        chmod u+s target/debug/fuselage
    else
        sudo chown root:root target/debug/fuselage
        sudo chmod u+s target/debug/fuselage
    fi

setuid-release:
    #!/usr/bin/env bash
    if [ "$(id -u)" = "0" ]; then
        chown root:root target/release/fuselage
        chmod u+s target/release/fuselage
    else
        sudo chown root:root target/release/fuselage
        sudo chmod u+s target/release/fuselage
    fi

install:
    cargo install --path .


# Sign and push a release tag, triggering the GitHub Actions release workflow.
# Monitor the workflow run manually via: gh run list --workflow=release.yml
# Run just publish-release once CI has completed successfully.
# Usage: just draft-release v0.2.0
draft-release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    # Refuse to tag a dirty working tree — uncommitted changes would not be part of the release.
    if ! git diff --quiet || ! git diff --cached --quiet; then
        echo "error: working tree has uncommitted changes; commit or stash them before tagging." >&2
        exit 1
    fi
    echo "Signing and pushing tag {{VERSION}}..."
    git tag -s "{{VERSION}}" -m "Release {{VERSION}}"
    git push origin "{{VERSION}}"
    echo "Tag pushed. Monitor CI at: https://github.com/sfkleach/fuselage/actions"
    echo "Run 'just publish-release {{VERSION}}' once the workflow completes successfully."

# Publish a release: push to crates.io (stable only) and publish the GitHub release.
# Run this after draft-release and once CI has completed successfully.
# Usage: just publish-release v0.2.0
publish-release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    # Only publish to crates.io for stable releases (no - suffix).
    # Pre-release (-rc) and draft (-) tags are not published to crates.io.
    if [[ "{{VERSION}}" != *"-"* ]]; then
        echo "Publishing to crates.io..."
        cargo publish --allow-dirty
    else
        echo "Skipping crates.io publish for non-stable tag {{VERSION}}."
    fi
    # Flip the GitHub release to published (or pre-release for -rc tags).
    if [[ "{{VERSION}}" == *"-rc"* ]]; then
        gh release edit "{{VERSION}}" --repo sfkleach/fuselage --draft=false --prerelease
    elif [[ "{{VERSION}}" == *"-"* ]]; then
        echo "NOTE: {{VERSION}} is a draft tag — not flipping to published."
    else
        gh release edit "{{VERSION}}" --repo sfkleach/fuselage --draft=false
    fi
    echo "Released {{VERSION}}."

# Initialize decision records
init-decisions:
    python3 scripts/decisions.py --init

# Add a new decision record
add-decision TOPIC:
    python3 scripts/decisions.py --add "{{TOPIC}}"
