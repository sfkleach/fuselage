# Contributing to fuselage

## Prerequisites

Building and testing fuselage requires the following tools.

### Quick setup (Debian/Ubuntu)

```bash
bash scripts/jumpstart.sh
```

This installs everything listed below in one step.

### Manual setup

| Tool | Package / installer | Purpose |
|------|---------------------|---------|
| Rust ≥ 1.85 (stable) | [rustup.rs](https://rustup.rs) | Build the Rust binaries |
| `gcc` | `apt install build-essential` | Compile the self-executing stub |
| `mksquashfs` | `apt install squashfs-tools` | Create squashfs archives |
| `uv` | [astral.sh/uv](https://astral.sh/uv) | Sync Python dependencies in `uv-bundle` |
| `just` | `cargo install just` | Task runner for build/test recipes |
| `musl-gcc` *(optional)* | `apt install musl-tools` | Produce smaller stubs (~50 KB vs ~700 KB) |

## Building

```bash
cargo build           # debug build
cargo build --release # release build
```

## Testing

```bash
just test             # unit tests + lint + format check + audit + functest (debug + setuid)
just unittest         # cargo test only
just functest-debug   # functional tests against the debug build (plain mode)
just functest-setuid  # functional tests against a setuid release build
```

The setuid functional tests require `fuselage` to be setuid-root. `just functest-setuid`
handles this automatically, using `sudo` to set the owner and the setuid bit.

## Code style

See [CLAUDE.md](CLAUDE.md) for the full style guide. Key points:

- 4-space indentation, spaces only, LF line endings.
- Maximum line length: 120 characters (first-to-last non-whitespace).
- Comments are proper sentences (capital letter, period), except short phrases or bullet points.

## Design decisions

Significant design choices are recorded in [docs/decisions/](docs/decisions/) using
the template in that directory.
