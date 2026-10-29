"""Prints every workspace member crates.io will receive, one per line.

What `publish:check` packages, read from `cargo metadata` rather than named, so
a member that sets `publish = false` (reported as an empty registry list) is
left out by its own manifest. Exits non-zero when `cargo metadata` fails or
lists no publishable member.
"""

import json
import subprocess
import sys


def publishable(metadata):
    """The names of the members in `metadata` that `publish` does not forbid."""
    return [
        package["name"]
        for package in metadata["packages"]
        if package["publish"] != []
    ]


def main():
    listing = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        stdout=subprocess.PIPE,
        text=True,
        check=True,
    )
    names = publishable(json.loads(listing.stdout))
    if not names:
        print("cargo metadata listed no publishable package", file=sys.stderr)
        return 1
    print("\n".join(names))
    return 0


if __name__ == "__main__":
    sys.exit(main())
