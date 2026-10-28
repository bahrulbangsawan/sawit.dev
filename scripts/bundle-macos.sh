#!/bin/sh
# Builds "PG Desk.app" (with icon) next to the binaries:
#   scripts/bundle-macos.sh          # release build → target/release/PG Desk.app
#   scripts/bundle-macos.sh debug    # reuse a debug build
#   scripts/bundle-macos.sh --dmg    # release build + compressed PG-Desk.dmg
#   scripts/bundle-macos.sh --zip    # release build + PG-Desk-<version>-macos-arm64.zip
#                                    # (the file the in-app updater downloads)
set -eu
cd "$(dirname "$0")/.."

dmg=no
zip=no
while :; do
    case "${1:-}" in
        --dmg) dmg=yes; shift ;;
        --zip) zip=yes; shift ;;
        *) break ;;
    esac
done
profile=${1:-release}
if [ "$profile" = release ]; then
    cargo build --release -p pgdesk -p pgdesk-mcp
else
    cargo build -p pgdesk -p pgdesk-mcp
fi

# The workspace version (bumped by scripts/release.sh).
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out="target/$profile"
app="$out/PG Desk.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$out/pgdesk" "$out/pgdesk-mcp" "$app/Contents/MacOS/"
cp assets/logo/PGDesk.icns "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>PG Desk</string>
    <key>CFBundleDisplayName</key><string>PG Desk</string>
    <key>CFBundleIdentifier</key><string>dev.pgdesk.app</string>
    <key>CFBundleExecutable</key><string>pgdesk</string>
    <key>CFBundleIconFile</key><string>PGDesk</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
# Ad-hoc signature over the whole bundle (no Developer ID needed locally).
codesign --force --deep --sign - "$app" >/dev/null 2>&1 || true
if [ "$zip" = yes ]; then
    archive="$out/PG-Desk-$version-macos-arm64.zip"
    rm -f "$archive"
    ditto -c -k --keepParent "$app" "$archive"
    echo "$archive"
fi
if [ "$dmg" = yes ]; then
    # LZMA-compressed (ULMO), with an /Applications link to drag onto.
    stage="$out/dmg"
    rm -rf "$stage" "$out/PG-Desk.dmg"
    mkdir -p "$stage"
    cp -R "$app" "$stage/"
    ln -s /Applications "$stage/Applications"
    hdiutil create -quiet -volname "PG Desk" -srcfolder "$stage" -format ULMO "$out/PG-Desk.dmg"
    rm -rf "$stage"
    echo "$out/PG-Desk.dmg"
fi
echo "$app"
