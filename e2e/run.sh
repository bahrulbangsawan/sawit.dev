#!/bin/bash
# Sawit's end-to-end tests: each test starts its own Sawit in a throwaway home and drives its
# window through the macOS accessibility tree with agent-desktop. See README.md, "End-to-end tests".
#
#   e2e/run.sh                     every test, headless (no pointer moves)
#   e2e/run.sh --filter canvas     the tests whose name matches (a regular expression)
#   e2e/run.sh --headed            also the tests that move the real pointer (drags, right-clicks)
#   e2e/run.sh --keep              keep every test's artifacts, not only the failed ones'
#   e2e/run.sh --no-build          use target/debug/sawit as it is
#   e2e/run.sh --idle 120          wait for 120 s without input first (default 60; 0: don't wait)
set -euo pipefail

E2E="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$E2E")"
AD="$E2E/node_modules/agent-desktop/bin/agent-desktop"

FILTER=""
HEADED=0
KEEP=0
BUILD=1
IDLE="${SAWIT_E2E_IDLE:-60}"
while [ $# -gt 0 ]; do
    case "$1" in
        --filter) FILTER="$2"; shift 2 ;;
        --filter=*) FILTER="${1#--filter=}"; shift ;;
        --headed) HEADED=1; shift ;;
        --keep) KEEP=1; shift ;;
        --no-build) BUILD=0; shift ;;
        --idle) IDLE="$2"; shift 2 ;;
        --no-idle-wait) IDLE=0; shift ;;
        -h|--help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "run.sh: unknown option $1 (see --help)" >&2; exit 2 ;;
    esac
done

say() { printf '\033[1m%s\033[0m\n' "$*"; }

# The suite's own dependencies, from the lockfile, nothing global.
if [ ! -x "$AD" ]; then
    say "Installing the suite's dependencies (npm ci)…"
    (cd "$E2E" && npm ci --no-audit --no-fund)
fi

# agent-desktop reads other apps through the Accessibility permission of the app that runs it
# (the terminal). It never asks for it: the person grants it in System Settings.
PERMS="$("$AD" permissions)"
if ! printf '%s' "$PERMS" | grep -q '"accessibility":{"state":"granted"'; then
    echo "agent-desktop has no Accessibility permission." >&2
    echo "Grant it in System Settings > Privacy & Security > Accessibility to the app this runs in" >&2
    echo "(Terminal, iTerm2, VS Code, …), and to $AD if it is listed separately, then run again." >&2
    exit 3
fi
if ! printf '%s' "$PERMS" | grep -q '"screen_recording":{"state":"granted"'; then
    echo "note: without Screen Recording, failed tests keep no screenshot (System Settings >" >&2
    echo "Privacy & Security > Screen & System Audio Recording, for the app this runs in)." >&2
fi

if [ "$BUILD" = 1 ]; then
    say "Building target/debug/sawit…"
    (cd "$REPO" && CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" cargo build -p sawit)
fi

RUN_ID="$(date +%Y%m%d-%H%M%S)"
ARTIFACTS="$E2E/artifacts/$RUN_ID"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/sawit-e2e-run.XXXXXX")"
TMP="$(cd "$TMP" && pwd -P)"
mkdir -p "$ARTIFACTS"
# agent-desktop refuses a state folder others can read: its snapshots hold what the window shows.
mkdir -m 700 "$TMP/agent-desktop"
export SAWIT_E2E_RUN_ID="$RUN_ID" SAWIT_E2E_ARTIFACTS="$ARTIFACTS" SAWIT_E2E_HEADED="$HEADED"
export SAWIT_E2E_KEEP="$KEEP" SAWIT_E2E_IDLE="$IDLE"
# agent-desktop keeps its snapshots here for this run, not in ~/.agent-desktop.
export AGENT_DESKTOP_HOME="$TMP/agent-desktop"

# The real drag between apps needs a drag source of our own (tools/drag-source.swift).
if [ "$HEADED" = 1 ] && command -v swiftc >/dev/null 2>&1; then
    if swiftc -O -o "$TMP/drag-source" "$E2E/tools/drag-source.swift" 2>"$TMP/swiftc.log"; then
        export SAWIT_E2E_DRAG_SOURCE="$TMP/drag-source"
    else
        echo "note: tools/drag-source.swift didn't build; the drop test will skip ($TMP/swiftc.log)" >&2
    fi
fi

node_eval() { node --input-type=module -e "$1"; }
FRONT_PID="$(node_eval "import { frontApp } from '$E2E/lib/host.js'; console.log((await frontApp()).pid)" || true)"

cleanup() {
    status=$?
    # Anything this run started that is still there, found by its environment (`ps eww`): a Sawit
    # the harness could not kill (each starts with SAWIT_E2E_RUN=<run id>), and the command host
    # of this checkout's agent-desktop (it would otherwise idle for 15 minutes).
    ps eww -axo pid=,command= 2>/dev/null | while read -r pid line; do
        case "$line" in
            *"SAWIT_E2E_RUN=$RUN_ID "*|*"SAWIT_E2E_RUN=$RUN_ID")
                kill "$pid" 2>/dev/null || true ;;
            "$AD "*AGENT_DESKTOP_INTERNAL_COMMAND_HOST=1*)
                kill "$pid" 2>/dev/null || true ;;
        esac
    done
    if [ -n "$FRONT_PID" ]; then
        node_eval "import { activate } from '$E2E/lib/host.js'; await activate($FRONT_PID)" || true
    fi
    # The temp worlds of tests that were interrupted before their teardown (lib/fixture.js).
    rm -rf "$TMP" "${TMPDIR:-/tmp}"/se-"$RUN_ID"-*
    rmdir "$ARTIFACTS" 2>/dev/null && rmdir "$E2E/artifacts" 2>/dev/null || true
    if [ -d "$ARTIFACTS" ]; then say "Artifacts: $ARTIFACTS"; fi
    exit $status
}
trap cleanup EXIT INT TERM

if [ "$IDLE" != 0 ]; then
    node_eval "import { waitForIdle } from '$E2E/lib/host.js';
        await waitForIdle($IDLE, { log: (l) => console.log(l), timeoutS: 3600 })"
fi

say "Running the end-to-end tests (run $RUN_ID)…"
ARGS=(--test --test-concurrency=1 --test-reporter=spec)
if [ -n "$FILTER" ]; then ARGS+=("--test-name-pattern=$FILTER"); fi
cd "$E2E"
set +e
node "${ARGS[@]}" specs/*.test.js 2>&1 | tee "$ARTIFACTS/run.log"
STATUS=${PIPESTATUS[0]}
set -e
# The log is worth keeping only beside a failure's artifacts.
if [ "$STATUS" = 0 ] && [ "$KEEP" = 0 ]; then rm -f "$ARTIFACTS/run.log"; fi
exit "$STATUS"
