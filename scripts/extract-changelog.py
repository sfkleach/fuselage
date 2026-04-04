#!/usr/bin/env python3
"""Extract the release notes for a given version tag from CHANGELOG.md.

Usage: extract-changelog.py <tag>

Prints the body of the first ## section whose heading contains the version
number extracted from the tag (i.e. the tag with a leading 'v' stripped).
For pre-release tags (e.g. v0.1.0-rc1) the pre-release suffix is stripped
and the base version (e.g. 0.1.0) is used to locate the section.
Exits with a non-zero status if no matching section is found, or if the first
## heading does not match (to catch version/tag mismatches early).
"""
import re
import sys


def main() -> None:
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <tag>", file=sys.stderr)
        sys.exit(1)

    tag = sys.argv[1]
    version = tag.lstrip("v")
    # Strip any pre-release suffix (e.g. "0.1.0-rc1" -> "0.1.0") so that a
    # pre-release tag matches the corresponding stable CHANGELOG section.
    base_version = version.split("-", 1)[0]

    with open("CHANGELOG.md") as f:
        content = f.read()

    # Split on level-2 headings.
    parts = re.split(r"\n(?=## )", content)
    for part in parts:
        if not part.startswith("## "):
            continue
        heading_end = part.index("\n") if "\n" in part else len(part)
        heading = part[:heading_end]
        body = part[heading_end:].strip()
        # Accept the heading if it contains the full version, the tag, or the
        # base version (for pre-release tags).
        if version in heading or tag in heading or base_version in heading:
            print(body)
            sys.exit(0)
        else:
            print(
                f"ERROR: First ## heading '{heading}' does not contain "
                f"version '{version}' or base version '{base_version}'.",
                file=sys.stderr,
            )
            sys.exit(1)

    print("ERROR: No ## heading found in CHANGELOG.md.", file=sys.stderr)
    sys.exit(1)


if __name__ == "__main__":
    main()
