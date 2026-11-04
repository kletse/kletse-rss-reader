#!/bin/bash
# Builds "Kletse RSS.app" for macOS from this checkout: the Rust core as a static library (src/ffi.rs), the
# SwiftUI app around it (macos/), the icon and an Info.plist. Needs Xcode (or its command line tools) and Rust;
# no Qt.
#
# Usage: packaging/macos/build-app.sh            → packaging/Kletse RSS.app
set -euo pipefail
cd "$(dirname "$0")/../.."

# The Swift app's minimum; the library is built for the same one, or the linker warns about every object.
export MACOSX_DEPLOYMENT_TARGET=15.0

cargo rustc --lib --release --locked --no-default-features --features ffi --crate-type staticlib
swift build -c release --package-path macos
version=$(packaging/check-version.sh)
app="packaging/Kletse RSS.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$(swift build -c release --package-path macos --show-bin-path)/KletseRSS" "$app/Contents/MacOS/kletse-rss"
# macOS 26 and later take the icon from Assets.car, with a light and a dark version, and switch by themselves. Older
# systems, and Xcode too old to compile a .icon, get the light one from AppIcon.icns.
cp packaging/macos/icon.icns "$app/Contents/Resources/AppIcon.icns"
xcrun actool packaging/logo/AppIcon.icon --compile "$app/Contents/Resources" --app-icon AppIcon --platform macosx \
  --target-device mac --minimum-deployment-target "$MACOSX_DEPLOYMENT_TARGET" \
  --output-partial-info-plist /dev/null >/dev/null 2>&1 || true
cp packaging/logo/icon-orange.png "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Kletse RSS</string>
  <key>CFBundleDisplayName</key><string>Kletse RSS</string>
  <key>CFBundleIdentifier</key><string>be.kletse.rss</string>
  <key>CFBundleExecutable</key><string>kletse-rss</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleIconName</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.news</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
# Signed ad hoc, as the linker signs the bare binary, so the bundle's seal matches what is in it.
codesign --force --sign - "$app"
echo "Built $app"
