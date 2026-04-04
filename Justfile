default:
    @just --list

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


# Initialize decision records
init-decisions:
    python3 scripts/decisions.py --init

# Add a new decision record
add-decision TOPIC:
    python3 scripts/decisions.py --add "{{TOPIC}}"
