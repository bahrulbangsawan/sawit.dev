#!/bin/sh
# Builds "PG Desk.app" (with icon) next to the binaries:
#   scripts/bundle-macos.sh          # release build → target/release/PG Desk.app
#   scripts/bundle-macos.sh debug    # reuse a debug build
set -eu
cd "$(dirname "$0")/.."

profile=${1:-release}
if [ "$profile" = release ]; then
    cargo build --release -p pgdesk -p pgdesk-mcp
else
    cargo build -p pgdesk -p pgdesk-mcp
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/app/Cargo.toml | head -1)
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
echo "$app"
