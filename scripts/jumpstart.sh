#!/usr/bin/env bash
# Install all prerequisites for building and testing fuselage on Debian/Ubuntu.
# Safe to run more than once — each step checks whether the tool is already present.
set -euo pipefail

# Require a Debian/Ubuntu system.
if ! command -v apt-get >/dev/null 2>&1; then
    echo "error: this script requires apt-get (Debian/Ubuntu only)" >&2
    exit 1
fi

echo "==> Installing apt packages (requires sudo)..."
sudo apt-get update -q
sudo apt-get install -y \
    build-essential \
    squashfs-tools \
    musl-tools

# Rust: install via rustup if cargo is not already present.
if command -v cargo >/dev/null 2>&1; then
    echo "==> Rust already installed ($(cargo --version))."
else
    echo "==> Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    # Make cargo available in the current shell without requiring a new login.
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
    echo "==> Rust installed ($(cargo --version))."
fi

# Ensure the Rust toolchain meets the minimum required version (1.85, edition 2024).
RUST_VERSION=$(rustc --version | grep -oP '\d+\.\d+' | head -1)
RUST_MAJOR=$(echo "$RUST_VERSION" | cut -d. -f1)
RUST_MINOR=$(echo "$RUST_VERSION" | cut -d. -f2)
if [ "$RUST_MAJOR" -lt 1 ] || { [ "$RUST_MAJOR" -eq 1 ] && [ "$RUST_MINOR" -lt 85 ]; }; then
    echo "==> Updating Rust toolchain to stable (need >=1.85, have $RUST_VERSION)..."
    rustup update stable
fi

# just: install via cargo if not already present.
if command -v just >/dev/null 2>&1; then
    echo "==> just already installed ($(just --version))."
else
    echo "==> Installing just..."
    cargo install just
    echo "==> just installed."
fi

# uv: install via the official installer if not already present.
if command -v uv >/dev/null 2>&1; then
    echo "==> uv already installed ($(uv --version))."
else
    echo "==> Installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    echo "==> uv installed."
    echo "    You may need to restart your shell or run: source \$HOME/.local/bin/env"
fi

echo ""
echo "All prerequisites installed. You can now build with:"
echo "  cargo build --release"
echo "  just test"
