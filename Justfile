default:
    @just --list

test: unittest lint fmt-check audit build functest

unittest:
    cargo test

lint:
    cargo clippy -- -D warnings

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

build:
    cargo build

build-release:
    cargo build --release
    ls -alh target/release/fuselage

functest-plain:
    # Functional tests for the plain binary
    rm -f target/debug/fuselage
    cargo build
    # Run functional tests on target/debug/fuselage
    # ...

functest-setuid:
    # Functional tests for the setuid binary
    rm -f target/release/fuselage
    cargo build --release
    sudo chown root:root target/release/fuselage
    sudo chmod u+s target/release/fuselage
    # Run functional tests on target/release/fuselage
    # ...

functest: functest-plain functest-setuid

setuid:
    sudo chown root:root target/debug/fuselage
    sudo chmod u+s target/debug/fuselage

install:
    cargo install --path .


# Initialize decision records
init-decisions:
    python3 scripts/decisions.py --init

# Add a new decision record
add-decision TOPIC:
    python3 scripts/decisions.py --add "{{TOPIC}}"
