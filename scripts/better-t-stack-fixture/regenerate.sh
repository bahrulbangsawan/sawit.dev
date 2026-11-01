#!/bin/sh
# Regenerates crates/core/src/stack_builder/upstream.json, the answers the Rust port of
# Better-T-Stack's builder is tested against, by running upstream's own TypeScript:
#   scripts/better-t-stack-fixture/regenerate.sh               # the commit the port is from
#   scripts/better-t-stack-fixture/regenerate.sh <commit>      # a newer one, when re-porting
# Needs gh (signed in) and bun. Upstream's files go to a temporary folder; only gen.ts and the
# stubs here (for the site URL, the URL keys and @better-t-stack/types' name schema) are ours.
set -eu
cd "$(dirname "$0")"
commit=${1:-a9bd456ccc9d}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/lib" "$work/types"

fetch() {
    gh api "repos/AmanVarshney01/create-better-t-stack/contents/$1?ref=$commit" --jq .content |
        base64 -d >"$2"
}

for f in constant stack-validation stack-compatibility sanitize-stack-addons stack-utils stack-model types; do
    fetch "apps/web/src/lib/$f.ts" "$work/lib/$f.ts"
done
fetch packages/types/src/compatibility.ts "$work/types/compatibility.ts"
fetch packages/types/src/constants.ts "$work/types/constants.ts"
cp stubs/lib/*.ts "$work/lib/"
cp stubs/types/index.ts "$work/types/index.ts"
cp gen.ts tsconfig.json "$work/"

out=../../crates/core/src/stack_builder/upstream.json
(cd "$work" && bun run gen.ts) | sed "s/\"commit\": \"a9bd456ccc9d\"/\"commit\": \"$commit\"/" >"$out"
echo "wrote $out from upstream $commit"
