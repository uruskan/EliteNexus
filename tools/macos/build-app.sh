#!/bin/bash
# Builds Elite Nexus for this Mac (Apple Silicon or Intel) and packs it as "Elite Nexus.app".
#
#   tools/macos/build-app.sh             build target/macos/Elite Nexus.app and a zip of it
#   tools/macos/build-app.sh --install   ... and copy the app to /Applications (~/Applications if that isn't writable)
#
# Needs Xcode's Command Line Tools (xcode-select --install) and Rust (https://rustup.rs). Honours CARGO_TARGET_DIR.
# The app keeps its data in ~/Library/Application Support/EliteNexus, never inside the bundle, so replacing the app
# keeps your settings, galaxy database and upload queues.
set -euo pipefail

install=0
for a in "$@"; do
  case "$a" in
    --install) install=1 ;;
    *) echo "Unknown option: $a (use --install)" >&2; exit 2 ;;
  esac
done

if [ "$(uname -s)" != Darwin ]; then
  echo "This script makes the macOS app and runs on a Mac." >&2
  exit 1
fi
for tool in cargo codesign iconutil ditto plutil; do
  command -v "$tool" >/dev/null || { echo "Missing $tool: install Xcode's Command Line Tools (xcode-select --install) and Rust (https://rustup.rs)." >&2; exit 1; }
done

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)"
arch="$(uname -m)"   # arm64 or x86_64
target_dir="${CARGO_TARGET_DIR:-$root/target}"
out="$target_dir/macos"
app="$out/Elite Nexus.app"

echo "==> Building Elite Nexus $version for macOS ($arch)"
cargo build --release -p nexus-app

echo "==> Packing $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$target_dir/release/EliteNexus" "$app/Contents/MacOS/EliteNexus"

# The icon is drawn in code (crates/nexus-app/src/icon.rs); iconutil turns the rendered sizes into an .icns.
iconset="$out/EliteNexus.iconset"
rm -rf "$iconset"
cargo run --quiet --release -p nexus-app --example macos_iconset -- "$iconset" >/dev/null
iconutil -c icns -o "$app/Contents/Resources/EliteNexus.icns" "$iconset"
rm -rf "$iconset"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleDisplayName</key><string>Elite Nexus</string>
  <key>CFBundleName</key><string>Elite Nexus</string>
  <key>CFBundleExecutable</key><string>EliteNexus</string>
  <key>CFBundleIconFile</key><string>EliteNexus</string>
  <key>CFBundleIdentifier</key><string>io.github.uruskan.EliteNexus</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.games</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSHumanReadableCopyright</key><string>Free software under the GNU GPL, version 3 or later. Elite Dangerous is a trademark of Frontier Developments plc; Elite Nexus is an unofficial community tool.</string>
</dict>
</plist>
PLIST
plutil -lint -s "$app/Contents/Info.plist"

# The licences travel with the program (GPL for Elite Nexus, SIL OFL for the bundled Inter font).
cp LICENSE LICENSE-ADDITIONAL-TERMS.md NAME-POLICY.md NOTICE.md README.md "$app/Contents/Resources/"
cp docs/USER_GUIDE.md "$app/Contents/Resources/USER_GUIDE.md"
cp data/fonts/inter/LICENSE.txt "$app/Contents/Resources/Inter-LICENSE.txt"

# An ad-hoc signature (no Apple developer account): enough for a Mac to run an app built on it.
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict "$app"

zip="$out/EliteNexus-v$version-macos-$arch.zip"
rm -f "$zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$zip"
echo "==> Built $app"
echo "    and $zip"

if [ "$install" = 1 ]; then
  dest=/Applications
  if [ ! -w "$dest" ]; then
    dest="$HOME/Applications"
    mkdir -p "$dest"
  fi
  if pgrep -xq EliteNexus; then
    echo "    Elite Nexus is running: quit it, then run this again to install." >&2
    exit 3
  fi
  rm -rf "$dest/Elite Nexus.app"
  ditto "$app" "$dest/Elite Nexus.app"
  echo "==> Installed $dest/Elite Nexus.app"
fi
