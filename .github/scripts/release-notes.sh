#!/usr/bin/env bash
# Prints the CHANGELOG.md section of the given version (without its heading).
# Used for the GitHub release body and for the notes Velopack shows in the
# in-app update dialog.
set -euo pipefail
version="${1:?usage: release-notes.sh <version>}"
notes="$(awk -v ver="${version}" '
  /^## / { printing = index($0, "[" ver "]") > 0; next }
  printing { print }
' CHANGELOG.md)"
if [[ -z "${notes//[[:space:]]/}" ]]; then
  echo "Release notes for ${version} were not found in CHANGELOG.md; using a pointer instead." >&2
  notes="Siehe CHANGELOG.md."
fi
printf '%s\n' "${notes}"
