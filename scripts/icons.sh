#!/usr/bin/env bash
# Renders assets/src/*.svg to the committed PNGs and assets/AppIcon.icns.
# Needs rsvg-convert (`brew install librsvg`) and iconutil (Xcode command line tools).
# Run after editing an SVG; builds use the committed outputs and do not need this script.
set -euo pipefail
cd "$(dirname "$0")/.."

rsvg-convert -w 44 -h 44 assets/src/tray-on.svg -o assets/tray-on.png
rsvg-convert -w 44 -h 44 assets/src/tray-off.svg -o assets/tray-off.png

iconset=$(mktemp -d)/AppIcon.iconset
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  rsvg-convert -w "$size" -h "$size" assets/src/app-icon.svg -o "$iconset/icon_${size}x${size}.png"
  rsvg-convert -w $((size * 2)) -h $((size * 2)) assets/src/app-icon.svg \
    -o "$iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$iconset" -o assets/AppIcon.icns
rsvg-convert -w 256 -h 256 assets/src/app-icon.svg -o assets/icon-256.png
rm -rf "$(dirname "$iconset")"
echo "icons written to assets/"
