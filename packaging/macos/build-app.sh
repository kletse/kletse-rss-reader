#!/bin/bash
# Builds "Kletse RSS.app" for macOS from this checkout: the release binary, the icon and an Info.plist.
# With Homebrew's Qt (`brew install qt`), macdeployqt then copies the Qt frameworks and QML modules the app
# uses into the bundle, so it keeps working when Homebrew updates or removes Qt.
#
# Usage: packaging/macos/build-app.sh            → packaging/Kletse RSS.app
set -euo pipefail
cd "$(dirname "$0")/../.."

cargo build --release --locked
version=$(packaging/check-version.sh)
app="packaging/Kletse RSS.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/kletse-rss "$app/Contents/MacOS/kletse-rss"
cp packaging/macos/icon.icns "$app/Contents/Resources/icon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Kletse RSS</string>
  <key>CFBundleDisplayName</key><string>Kletse RSS</string>
  <key>CFBundleIdentifier</key><string>be.kletse.rss</string>
  <key>CFBundleExecutable</key><string>kletse-rss</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.news</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

if command -v macdeployqt >/dev/null; then
  macdeployqt "$app" -qmldir=qml -always-overwrite
else
  echo "macdeployqt not found: the app uses the Qt it was built against (fine on this Mac)." >&2
fi
echo "Built $app"
