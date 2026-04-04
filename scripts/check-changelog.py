#!/usr/bin/env python3
"""Check that the top section of CHANGELOG.md is a version release.

Usage: check-changelog.py

Exits with a non-zero status if the first ## section heading contains the word
'unreleased' (case-insensitive), indicating the release is not yet ready.
"""
import re
import sys


def main() -> None:
    with open("CHANGELOG.md") as f:
        content = f.read()

    # Split on level-2 headings.
    parts = re.split(r"\n(?=## )", content)
    for part in parts:
        if not part.startswith("## "):
            continue
        heading_end = part.index("\n") if "\n" in part else len(part)
        heading = part[:heading_end]
        if "unreleased" in heading.lower():
            print(
                f"FAIL: Top CHANGELOG section is '{heading}' "
                f"— release is not ready."
            )
            sys.exit(1)
        else:
            print(f"OK: Top CHANGELOG section is '{heading}'.")
            sys.exit(0)

    print("FAIL: No ## heading found in CHANGELOG.md.")
    sys.exit(1)


if __name__ == "__main__":
    main()
