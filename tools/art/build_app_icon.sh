#!/bin/bash
# Compile the Liquid Glass app icon and cut the flat fallbacks from it.
#
# Usage: tools/art/build_app_icon.sh   (from anywhere; needs Xcode 26 or later)
#
# Source: src-tauri/icons/AppIcon.icon (Icon Composer bundle; its layers come from
# tools/art/app_icon_layers.py). Writes, in src-tauri/icons/:
#   Assets.car     the layered icon; the bundle ships it as Contents/Resources/Assets.car
#                  (tauri.conf.json bundle.macOS.files) and Info.plist names it with
#                  CFBundleIconName = AppIcon. macOS 26 and later draw this one.
#   icon.icns      actool's fallback for older macOS (CFBundleIconFile)
#   32x32.png 128x128.png 128x128@2x.png icon.png
#                  the default rendition, on the macOS icon grid, for the window icon
#                  and other platforms
# Run it after any change to AppIcon.icon, and commit the outputs.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
icons="$root/src-tauri/icons"
src="$icons/AppIcon.icon"
ictool="$(xcode-select -p)/../Applications/Icon Composer.app/Contents/Executables/ictool"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

xcrun actool "$src" --compile "$tmp" --app-icon AppIcon \
  --platform macosx --minimum-deployment-target 13.0 \
  --output-partial-info-plist "$tmp/partial.plist" >/dev/null
cp "$tmp/Assets.car" "$icons/Assets.car"
cp "$tmp/AppIcon.icns" "$icons/icon.icns"

# ictool draws the tile edge to edge; the macOS grid puts an 824 px tile in a 1024 canvas.
"$ictool" "$src" --export-image --output-file "$tmp/full.png" --platform macOS \
  --rendition Default --width 1024 --height 1024 --scale 1 >/dev/null
sips -z 824 824 "$tmp/full.png" --out "$tmp/tile.png" >/dev/null
sips -p 1024 1024 "$tmp/tile.png" --out "$tmp/grid.png" >/dev/null
for spec in 32:32x32 128:128x128 256:128x128@2x 512:icon; do
  sips -z "${spec%%:*}" "${spec%%:*}" "$tmp/grid.png" --out "$icons/${spec#*:}.png" >/dev/null
done
echo "wrote Assets.car, icon.icns and the PNG fallbacks in $icons"
