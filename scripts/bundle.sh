#!/usr/bin/env bash
# Builds Caffeinous.app (+ dmg, or pkg for the App Store) into dist/.
#
#   scripts/bundle.sh                        # direct build for this Mac's architecture
#   scripts/bundle.sh --universal            # arm64 + x86_64
#   scripts/bundle.sh --appstore --universal # sandboxed, no lid feature, signed .pkg
#
# Signing is optional and driven by environment variables:
#   SIGN_IDENTITY       "Developer ID Application: …" (direct) or "Apple Distribution: …" (appstore)
#   INSTALLER_IDENTITY  "3rd Party Mac Developer Installer: …" (appstore .pkg only)
#   PROVISIONING_PROFILE path to the Mac App Store .provisionprofile (appstore only)
# Without SIGN_IDENTITY the app is ad-hoc signed, which runs locally but not after download.
set -euo pipefail
cd "$(dirname "$0")/.."

mode=direct
universal=0
for arg in "$@"; do
  case "$arg" in
    --appstore) mode=appstore ;;
    --universal) universal=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

version=$(cargo metadata --no-deps --format-version 1 \
  | jq -r '.packages[] | select(.name == "caffeinous-app") | .version')
build=${BUILD_NUMBER:-$(git rev-list --count HEAD 2>/dev/null || echo 1)}
# Always non-empty: macOS ships bash 3.2, where "${empty[@]}" fails under `set -u`.
features=(--features "")
[[ $mode == appstore ]] && features=(--no-default-features)

if [[ $universal == 1 ]]; then
  targets=(aarch64-apple-darwin x86_64-apple-darwin)
else
  targets=("$(rustc -vV | sed -n 's/^host: //p')")
fi

bins=()
for t in "${targets[@]}"; do
  cargo build --release --locked -p caffeinous-app --target "$t" "${features[@]}"
  bins+=("target/$t/release/caffeinous-app")
  if [[ $mode == direct ]]; then
    cargo build --release --locked -p caffeinous-cli --target "$t"
  fi
done

out=dist
app=$out/Caffeinous.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create "${bins[@]}" -output "$app/Contents/MacOS/Caffeinous"
sed -e "s/__VERSION__/$version/" -e "s/__BUILD__/$build/" app/Info.plist > "$app/Contents/Info.plist"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
printf 'APPL????' > "$app/Contents/PkgInfo"

if [[ $mode == appstore ]]; then
  : "${PROVISIONING_PROFILE:?set PROVISIONING_PROFILE for an App Store build}"
  cp "$PROVISIONING_PROFILE" "$app/Contents/embedded.provisionprofile"
fi

entitlements=app/entitlements/$mode.plist
if [[ -n ${SIGN_IDENTITY:-} ]]; then
  codesign --force --timestamp --options runtime --entitlements "$entitlements" \
    --sign "$SIGN_IDENTITY" "$app"
else
  codesign --force --sign - --entitlements "$entitlements" "$app"
fi
codesign --verify --strict --verbose=2 "$app"

if [[ $mode == appstore ]]; then
  : "${INSTALLER_IDENTITY:?set INSTALLER_IDENTITY to build the App Store .pkg}"
  pkg=$out/Caffeinous-$version.pkg
  productbuild --component "$app" /Applications --sign "$INSTALLER_IDENTITY" "$pkg"
  echo "$pkg"
  exit 0
fi

arch=$([[ $universal == 1 ]] && echo universal || uname -m)

# Command-line tool, shipped next to the dmg.
cli_bins=()
for t in "${targets[@]}"; do cli_bins+=("target/$t/release/caffeinous"); done
cli_dir=$(mktemp -d)
lipo -create "${cli_bins[@]}" -output "$cli_dir/caffeinous"
if [[ -n ${SIGN_IDENTITY:-} ]]; then
  codesign --force --timestamp --options runtime --sign "$SIGN_IDENTITY" "$cli_dir/caffeinous"
fi
tar -C "$cli_dir" -czf "$out/caffeinous-cli_${version}_${arch}.tar.gz" caffeinous
rm -rf "$cli_dir"

dmg=$out/Caffeinous_${version}_${arch}.dmg
stage=$(mktemp -d)
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -volname Caffeinous -srcfolder "$stage" -ov -format UDZO "$dmg" >/dev/null
rm -rf "$stage"
if [[ -n ${SIGN_IDENTITY:-} ]]; then
  codesign --force --timestamp --sign "$SIGN_IDENTITY" "$dmg"
fi
echo "$app"
echo "$dmg"
