#!/bin/sh
# Fails when a hand-written source file is longer than the line limit (500, or $LINE_LIMIT).
# Every line counts, as `wc -l` counts them: imports, comments and blank lines too.
#
#   scripts/check-line-limit.sh            every source file in the repository
#   scripts/check-line-limit.sh crates/app only files under these paths
#
# Checks the files git knows (tracked, or new and not ignored), so build output and installed
# packages (target/, node_modules/, dist/) never count. Also skipped: vendored code, the asset
# folders (assets/ and each crate's assets/), and files a tool generates (the site's routeTree.gen.ts).
set -eu
limit=${LINE_LIMIT:-500}
cd "$(git rev-parse --show-toplevel)"
[ "$#" -gt 0 ] || set -- .
git ls-files -z --cached --others --exclude-standard -- "$@" |
  tr '\0' '\n' |
  grep -E '\.(rs|ts|tsx|js|jsx|mjs|cjs|css|html|sh|swift|sql|py)$' |
  grep -Ev '(^|/)(vendor|node_modules|target|dist)/|^assets/|^crates/[^/]+/assets/|(^|/)routeTree\.gen\.ts$' |
  sort -u |
  while IFS= read -r f; do
    [ -f "$f" ] && printf '%s\0' "$f"
  done |
  xargs -0 -r wc -l |
  awk -v limit="$limit" '
    # wc prints "<count> <path>" for each file, and a "<count> total" line per batch of files.
    { count = $1; sub(/^[ \t]*[0-9]+[ \t]/, "") }
    $0 == "total" { next }
    count > limit { printf "%6d  %s\n", count, $0; over++ }
    { n++ }
    END {
      if (over) { printf "%d of %d source files are over %d lines.\n", over, n, limit; exit 1 }
      printf "All %d source files are at most %d lines.\n", n, limit
    }'
