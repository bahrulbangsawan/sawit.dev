#!/bin/sh
# Builds "Sawit.app" (with icon) next to the binaries:
#   scripts/bundle-macos.sh          # release build → target/release/Sawit.app
#   scripts/bundle-macos.sh debug    # reuse a debug build
#   scripts/bundle-macos.sh --dmg    # release build + compressed Sawit.dmg
#   scripts/bundle-macos.sh --zip    # release build + Sawit-<version>-macos-arm64.zip
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
    cargo build --release -p sawit
else
    cargo build -p sawit
fi

# The workspace version (bumped by scripts/release.sh).
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out="target/$profile"
app="$out/Sawit.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$out/sawit" "$app/Contents/MacOS/"
# The MCP server is built into `sawit` (it runs as the server when started
# under this name), so the bundle doesn't carry a second copy of sawit-core.
ln -s sawit "$app/Contents/MacOS/sawit-mcp"
cp assets/logo/Sawit.icns "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Sawit</string>
    <key>CFBundleDisplayName</key><string>Sawit</string>
    <key>CFBundleIdentifier</key><string>dev.sawit.app</string>
    <key>CFBundleExecutable</key><string>sawit</string>
    <key>CFBundleIconFile</key><string>Sawit</string>
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
    archive="$out/Sawit-$version-macos-arm64.zip"
    rm -f "$archive"
    ditto -c -k --keepParent "$app" "$archive"
    echo "$archive"
fi
if [ "$dmg" = yes ]; then
    # LZMA-compressed (ULMO), with an /Applications link to drag onto.
    stage="$out/dmg"
    rm -rf "$stage" "$out/Sawit.dmg"
    mkdir -p "$stage"
    cp -R "$app" "$stage/"
    ln -s /Applications "$stage/Applications"
    hdiutil create -quiet -volname "Sawit" -srcfolder "$stage" -format ULMO "$out/Sawit.dmg"
    rm -rf "$stage"
    echo "$out/Sawit.dmg"
fi
echo "$app"
