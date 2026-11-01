#!/bin/bash
# usage: tools/crate-newer.sh <crate> <known-version>
# Exit 0 when crates.io reports a latest version of <crate> that differs from <known-version>; exit 1 when it is the same, and exit 1 when the
# lookup fails (a network error is not news). Used by the `WAIT:` lines in HANDOFF.md.
set -o pipefail
crate="$1"
known="$2"
latest="$(curl -fsS -A httpant-lane "https://crates.io/api/v1/crates/${crate}" | grep -o '"max_version":"[^"]*"' | head -n 1)" || exit 1
[ -n "$latest" ] || exit 1
[ "$latest" != "\"max_version\":\"${known}\"" ]
