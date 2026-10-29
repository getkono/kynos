#!/bin/sh
# Usage: sh scripts/refuses.sh WHEN-IT-BUILDS EXPECTED COMMAND...
#
# Asserts that COMMAND, a build that must be refused, fails with EXPECTED in
# its output. A COMMAND that succeeds prints WHEN-IT-BUILDS and fails; one that
# fails for another reason fails the `grep`.
set -eu

built="$1"
expected="$2"
shift 2

output="$(mktemp)"
trap 'rm -f "$output"' EXIT
if "$@" >"$output" 2>&1; then
  echo "$built" >&2
  exit 1
fi
grep -F "$expected" "$output"
