#!/bin/sh
# Rebuilds bundle/ (mesurer.js, gate.js, SHA256SUMS, BUILD.json) from mesurer's source at the
# pinned commit. Node and pnpm are needed here only, never for `cargo build`.
#
#   vendor/mesurer/rebuild.sh            rebuild bundle/
#   vendor/mesurer/rebuild.sh --check    rebuild elsewhere and compare with bundle/SHA256SUMS
#
# MESURER_SRC names a clone to build from (it must be at the pinned commit, with no changes);
# without it, the commit is cloned into $TMPDIR. No lifecycle script is run.
set -eu
COMMIT=5c2fdaefde54591329d94b179af6ef5f32e15676
REPO=https://github.com/ibelick/mesurer
here=$(cd "$(dirname "$0")" && pwd)
check=0
case "${1:-}" in
  --check) check=1 ;;
  "") ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

if [ -n "${MESURER_SRC:-}" ]; then
  src=$MESURER_SRC
else
  src=$(mktemp -d "${TMPDIR:-/tmp}/mesurer-src.XXXXXX")
  git clone --quiet --no-checkout "$REPO" "$src"
  git -C "$src" checkout --quiet "$COMMIT"
fi
head=$(git -C "$src" rev-parse HEAD)
if [ "$head" != "$COMMIT" ]; then
  echo "$src is at $head, not mesurer $COMMIT" >&2
  exit 1
fi
# Sawit uses mesurer as it is: a clone with changes to tracked files isn't built from.
if [ -n "$(git -C "$src" status --porcelain --untracked-files=no)" ]; then
  echo "$src has changes; Sawit builds mesurer unmodified" >&2
  exit 1
fi

(cd "$src" && pnpm install --frozen-lockfile --ignore-scripts --reporter=silent)
(cd "$src" && pnpm --filter mesurer run generate:styles >/dev/null)

out=$here/bundle
if [ "$check" = 1 ]; then
  out=$(mktemp -d "${TMPDIR:-/tmp}/mesurer-check.XXXXXX")
fi
MESURER_SRC=$src MESURER_OUT=$out PNPM_VERSION=$(pnpm --version) node "$here/build/build.mjs"

if [ "$check" = 1 ]; then
  if cmp -s "$here/bundle/SHA256SUMS" "$out/SHA256SUMS"; then
    echo "bundle/ matches a fresh build of mesurer $COMMIT."
  else
    echo "bundle/ differs from a fresh build of mesurer $COMMIT:" >&2
    diff "$here/bundle/SHA256SUMS" "$out/SHA256SUMS" >&2 || true
    exit 1
  fi
fi
