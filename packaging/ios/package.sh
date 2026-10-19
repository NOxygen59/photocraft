#!/usr/bin/env bash
# Build PhotoCraft for iPadOS 16+ and package it as a TrollStore .tipa (an IPA, ad-hoc signed;
# TrollStore applies its own signature on install).
#
# Needs: Xcode (iPhoneOS SDK), rustup with the aarch64-apple-ios target.
#
#   packaging/ios/package.sh               -> dist/PhotoCraft-<version>-ios16.tipa
#   packaging/ios/package.sh --simulator   -> dist/simulator/PhotoCraft.app (Apple silicon Macs)
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"
export IPHONEOS_DEPLOYMENT_TARGET=16.0
TARGET=aarch64-apple-ios
SIMULATOR=0
if [ "${1:-}" = "--simulator" ]; then
  TARGET=aarch64-apple-ios-sim
  SIMULATOR=1
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
SHORT_VERSION="${VERSION%%-*}"
BUILD_SHA="$(git rev-parse --short HEAD 2>/dev/null || echo unknown)"

rustup target add "$TARGET" >/dev/null
cargo build --release --locked --target "$TARGET" -p photocraft --features heif

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
APP="$STAGE/Payload/PhotoCraft.app"
mkdir -p "$APP"
cp "target/$TARGET/release/photocraft" "$APP/PhotoCraft"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT_VERSION/g" -e "s/@BUILD_SHA@/$BUILD_SHA/g" \
  packaging/ios/Info.plist.in >"$APP/Info.plist"
plutil -lint "$APP/Info.plist" >/dev/null
plutil -convert binary1 "$APP/Info.plist"
printf 'APPL????' >"$APP/PkgInfo"

# Loose icons (no asset catalog): iPhone 60 pt at 2x and 3x, iPad 76 pt and 83.5 pt at 2x.
ICON=assets/app-icon/photocraft-1024.png
sips -z 120 120 "$ICON" --out "$APP/AppIcon60x60@2x.png" >/dev/null
sips -z 180 180 "$ICON" --out "$APP/AppIcon60x60@3x.png" >/dev/null
sips -z 152 152 "$ICON" --out "$APP/AppIcon76x76@2x~ipad.png" >/dev/null
sips -z 167 167 "$ICON" --out "$APP/AppIcon83.5x83.5@2x~ipad.png" >/dev/null
sips -z 76 76 "$ICON" --out "$APP/AppIcon76x76~ipad.png" >/dev/null

# Licences ship with the app.
cp LICENSE-MIT LICENSE-APACHE NOTICE "$APP/"

strip -x "$APP/PhotoCraft" 2>/dev/null || true

if [ "$SIMULATOR" = 1 ]; then
  # Simulators of iOS 27 and later stop apps linked against their SDK that don't adopt the UIScene
  # life cycle, which winit 0.30 predates: the binary says it was linked against iOS 18 instead.
  # Only the simulator build: iOS 16 on a device doesn't make that check.
  xcrun vtool -set-build-version iossim 16.0 18.0 -replace -output "$APP/PhotoCraft.tmp" "$APP/PhotoCraft"
  mv -f "$APP/PhotoCraft.tmp" "$APP/PhotoCraft"
  codesign --force --sign - --timestamp=none "$APP"
  rm -rf dist/simulator && mkdir -p dist/simulator
  cp -R "$APP" dist/simulator/
  echo "$ROOT/dist/simulator/PhotoCraft.app"
  exit 0
fi
# iOS 27 stops apps linked against its SDK that don't adopt the UIScene life cycle, which winit
# 0.30 predates (recent iPhones run it): the binary says it was linked against iOS 18 instead, as
# the Simulator build above does. iOS 16 doesn't look.
xcrun vtool -set-build-version ios 16.0 18.0 -replace -output "$APP/PhotoCraft.tmp" "$APP/PhotoCraft"
mv -f "$APP/PhotoCraft.tmp" "$APP/PhotoCraft"
codesign --force --sign - --timestamp=none --entitlements packaging/ios/entitlements.plist "$APP"

mkdir -p dist
OUT="$ROOT/dist/PhotoCraft-$VERSION-ios16.tipa"
rm -f "$OUT"
(cd "$STAGE" && zip -qry "$OUT" Payload)
echo "$OUT"
