#!/bin/sh
# Cuts a release from the commits since the last tag: bumps the patch
# version (the very first release keeps 0.0.1), writes its CHANGELOG.md
# section from the commit subjects, commits "chore(release): vX" and tags
# vX. Called by .githooks/pre-push; prints the new tag, or nothing when
# there is nothing new to release. `release.sh --dry-run` prints the section
# it would write instead, and changes nothing.
#
# A release is what main holds, so it is cut on main only:
# SAWIT_RELEASE_ANY_BRANCH=1 cuts one on another branch on purpose.
set -eu
cd "$(git rev-parse --show-toplevel)"
dry_run=no
[ "${1:-}" != --dry-run ] || dry_run=yes

if branch=$(git symbolic-ref -q --short HEAD); then
    here="on $branch"
else
    here="on a detached HEAD"
fi
if [ "$here" != "on main" ] && [ -z "${SAWIT_RELEASE_ANY_BRANCH:-}" ]; then
    echo "release: releases are cut on main, and this checkout is $here: run it in a checkout of main (SAWIT_RELEASE_ANY_BRANCH=1 cuts one here anyway)" >&2
    exit 1
fi

for f in Cargo.toml Cargo.lock CHANGELOG.md; do
    if ! git diff --quiet HEAD -- "$f"; then
        echo "release: $f has uncommitted changes" >&2
        exit 1
    fi
done

current=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
last_tag=$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)
range=${last_tag:+$last_tag..}HEAD
subjects=$(git log --no-merges --format=%s "$range" | grep -v '^chore(release):' || true)
[ -n "$subjects" ] || exit 0

if git rev-parse -q --verify "refs/tags/v$current" >/dev/null; then
    next=$(echo "$current" | awk -F. '{ printf "%d.%d.%d", $1, $2, $3 + 1 }')
else
    next=$current
fi
# Checked before anything changes: otherwise the release commit is made and
# only its tag fails, which leaves that commit untagged on this branch.
if git rev-parse -q --verify "refs/tags/v$next" >/dev/null; then
    echo "release: tag v$next exists already, though Cargo.toml says $current, so v$next can't be cut; nothing was changed. If that tag was made by mistake and never pushed, delete it: git tag -d v$next" >&2
    exit 1
fi

# The notes go to the GitHub release, the updater and the in-app Changelog,
# so they list what users get in the app: docs, tests, refactors and build,
# CI and chore work (by type, or a docs scope) and the sawit.dev website (a
# site scope: it deploys on its own) are left out, unless that is all there is.
listed=$(echo "$subjects" | grep -Ev '^(docs|test|chore|ci|build|style|refactor)(\(|!?:)|^[a-z]+\((docs|site)\)!?:' || true)
[ -n "$listed" ] || listed=$subjects

notes=$(mktemp)
trap 'rm -f "$notes" "$notes.new"' EXIT
# "feat(app): x" → "- **app**: x", grouped by commit type.
group() {
    title=$1
    shift
    lines=$(echo "$listed" | grep -E "$@" | sed -E \
        -e 's/^[a-z]+\(([^)]*)\)!?: /**\1**: /' \
        -e 's/^[a-z]+!?: //' \
        -e 's/^/- /' || true)
    [ -z "$lines" ] || printf '### %s\n\n%s\n\n' "$title" "$lines" >>"$notes"
}
group "Features" '^feat'
group "Fixes" '^fix'
group "Performance" '^perf'
group "Other" -v '^(feat|fix|perf)'
if [ "$dry_run" = yes ]; then
    printf '## [%s] - %s\n\n' "$next" "$(date +%Y-%m-%d)"
    cat "$notes"
    exit 0
fi

# The new section goes above the previous ones, below the intro.
section=$(mktemp)
printf '## [%s] - %s\n\n' "$next" "$(date +%Y-%m-%d)" >"$section"
cat "$notes" >>"$section"
awk -v section="$section" '
    !done && /^## \[/ { while ((getline line < section) > 0) print line; done = 1 }
    { print }
    END { if (!done) { print ""; while ((getline line < section) > 0) print line } }
' CHANGELOG.md >"$notes.new"
mv "$notes.new" CHANGELOG.md
rm -f "$section"

if [ "$next" != "$current" ]; then
    awk -v v="$next" '!done && /^version = "/ { $0 = "version = \"" v "\""; done = 1 } { print }' \
        Cargo.toml >Cargo.toml.new && mv Cargo.toml.new Cargo.toml
    cargo update --workspace --offline --quiet
fi

git add Cargo.toml Cargo.lock CHANGELOG.md
git commit --quiet --no-verify -m "chore(release): v$next" -- Cargo.toml Cargo.lock CHANGELOG.md
# whitespace, not the default strip, which would drop the "### Features" headings.
git tag -a --cleanup=whitespace "v$next" -F "$notes"
echo "v$next"
