#!/bin/sh
# Installs the latest Sawit release into /Applications (or ~/Applications):
#   curl -fsSL https://raw.githubusercontent.com/bahrulbangsawan/sawit.dev/main/scripts/install.sh | sh
#   … | SAWIT_VERSION=v0.0.3 sh    # a specific release
# Files fetched by curl carry no quarantine flag, so Gatekeeper doesn't
# block the ad-hoc signed app the way it does a browser-downloaded DMG.
# Everything runs from `main`, called on the last line: piped into sh, a
# download cut off midway runs nothing rather than half the steps.
set -eu

REPO=bahrulbangsawan/sawit.dev
# The app's own process. The `sawit` command and `sawit-mcp` are the same
# binary under other names, and they keep running when the app quits.
APP='Sawit\.app/Contents/MacOS/sawit$'
# Seconds to wait for Sawit to quit.
QUIT_WAIT=30

say() { printf '%s\n' "$*"; }
die() { printf 'sawit-install: %s\n' "$*" >&2; exit 1; }

# True while Sawit runs, or while an update it began installing as it quit
# still swaps the app (that script waits on the app's PID, `$1`).
busy() {
    pgrep -f "$APP" >/dev/null 2>&1 && return 0
    for pid in $1; do
        pgrep -f "kill -0 $pid " >/dev/null 2>&1 && return 0
    done
    return 1
}

main() {
    case "$(uname -s)" in
        Darwin) ;;
        Linux) die "Sawit is macOS-only for now; there is no Linux build." ;;
        MINGW* | MSYS* | CYGWIN*) die "Sawit is macOS-only for now; there is no Windows build." ;;
        *) die "unsupported OS: $(uname -s)" ;;
    esac
    # `uname -m` says x86_64 under Rosetta, so ask the hardware instead.
    if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" != 1 ]; then
        die "Sawit needs an Apple Silicon Mac; there is no Intel build."
    fi

    if [ -n "${SAWIT_VERSION:-}" ]; then
        api="https://api.github.com/repos/$REPO/releases/tags/$SAWIT_VERSION"
    else
        api="https://api.github.com/repos/$REPO/releases/latest"
    fi
    url=$(curl -fsSL -H "Accept: application/vnd.github+json" "$api" |
        sed -n 's/.*"browser_download_url": *"\([^"]*-macos-arm64\.zip\)".*/\1/p' | head -1)
    [ -n "$url" ] || die "no macOS zip found in release ${SAWIT_VERSION:-latest}"

    if [ -w /Applications ]; then
        dest=/Applications
    else
        dest="$HOME/Applications"
        mkdir -p "$dest"
    fi

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    say "Downloading ${url##*/}…"
    curl -fL --progress-bar -o "$tmp/sawit.zip" "$url"
    ditto -x -k "$tmp/sawit.zip" "$tmp"
    [ -x "$tmp/Sawit.app/Contents/MacOS/sawit" ] || die "the archive has no Sawit.app"

    # Replace the app only once it has quit: deleting the bundle of a running
    # app, or one an update is swapping, can leave no app at all.
    pids=$(pgrep -f "$APP" || true)
    if [ -n "$pids" ]; then
        say "Quitting the running Sawit…"
        osascript -e 'quit app "Sawit"' >/dev/null 2>&1 || true
        waited=0
        while busy "$pids"; do
            [ "$waited" -lt "$QUIT_WAIT" ] || die "Sawit is still running. Quit it, then run this again."
            sleep 1
            waited=$((waited + 1))
        done
    fi
    rm -rf "$dest/Sawit.app"
    mv "$tmp/Sawit.app" "$dest/"
    # Clears the flag in case the zip itself was quarantined somewhere upstream.
    xattr -dr com.apple.quarantine "$dest/Sawit.app" 2>/dev/null || true

    say "Installed $dest/Sawit.app"

    say "Open it with: open \"$dest/Sawit.app\""
    say "For the \`sawit\` command, click Install CLI in the status bar at the bottom of the window."
}

main "$@"
