#!/usr/bin/env python3
"""Check that the top section of CHANGELOG.md is a version release and matches Cargo.toml.

Usage: check-changelog.py

Exits with a non-zero status if:
- The first ## section heading contains the word 'unreleased', or
- The version in the first ## heading does not match the version in Cargo.toml, or
- The version in Cargo.lock does not match the version in Cargo.toml.
"""
import re
import sys


def cargo_version() -> str:
    with open("Cargo.toml") as f:
        for line in f:
            m = re.match(r'^version\s*=\s*"([^"]+)"', line)
            if m:
                return m.group(1)
    print("FAIL: Could not find version in Cargo.toml.")
    sys.exit(1)


def lock_version() -> str:
    """Return the version of the top-level package recorded in Cargo.lock."""
    with open("Cargo.lock") as f:
        content = f.read()
    # Cargo.lock lists the workspace packages first; find the [[package]] block
    # for 'fuselage' and extract its version.
    m = re.search(r'\[\[package\]\]\s+name\s*=\s*"fuselage"\s+version\s*=\s*"([^"]+)"', content)
    if m:
        return m.group(1)
    print("FAIL: Could not find fuselage package entry in Cargo.lock.")
    sys.exit(1)


def main() -> None:
    with open("CHANGELOG.md") as f:
        content = f.read()

    # Collect all level-2 headings.
    parts = re.split(r"\n(?=## )", content)
    headings = []
    for part in parts:
        if not part.startswith("## "):
            continue
        heading_end = part.index("\n") if "\n" in part else len(part)
        headings.append(part[:heading_end])

    if not headings:
        print("FAIL: No ## heading found in CHANGELOG.md.")
        sys.exit(1)

    # Extract version strings (vX.Y.Z) from all headings and check for duplicates.
    seen: dict[str, str] = {}
    for heading in headings:
        m = re.search(r"v\d+\.\d+\.\d+", heading)
        if not m:
            continue
        ver = m.group(0)
        if ver in seen:
            print(
                f"FAIL: Version '{ver}' appears more than once in CHANGELOG.md "
                f"('{seen[ver]}' and '{heading}')."
            )
            sys.exit(1)
        seen[ver] = heading

    heading = headings[0]
    if "unreleased" in heading.lower():
        print(
            f"FAIL: Top CHANGELOG section is '{heading}' "
            f"— release is not ready."
        )
        sys.exit(1)

    version = cargo_version()
    if version not in heading:
        print(
            f"FAIL: Cargo.toml version '{version}' not found in "
            f"CHANGELOG heading '{heading}'."
        )
        sys.exit(1)

    locked = lock_version()
    if locked != version:
        print(
            f"FAIL: Cargo.lock version '{locked}' does not match "
            f"Cargo.toml version '{version}' — run 'cargo build' and commit Cargo.lock."
        )
        sys.exit(1)

    print(f"OK: Top CHANGELOG section is '{heading}' (matches Cargo.toml v{version}, Cargo.lock v{locked}).")
    sys.exit(0)


if __name__ == "__main__":
    main()
