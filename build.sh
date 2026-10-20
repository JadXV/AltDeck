#!/bin/bash
set -euo pipefail

echo "This builder is intended for me (JadXV) to build AltDeck for installation, if you are just trying to use AltDeck normally, run install.sh instead."

cd "$(dirname "$0")"

command -v cargo-tauri >/dev/null || cargo install tauri-cli --version "^2" --locked
rustup target add aarch64-apple-darwin x86_64-apple-darwin

rm -rf AltDeckCompressed AltDeckCompressed.zip
mkdir AltDeckCompressed

for target in aarch64-apple-darwin:ARM64 x86_64-apple-darwin:x86_64; do
  triple="${target%%:*}"
  suffix="${target##*:}"
  (cd src-tauri && cargo tauri build --target "$triple" --bundles app)
  cp -R "src-tauri/target/$triple/release/bundle/macos/AltDeck.app" "AltDeckCompressed/AltDeck-$suffix.app"
done

ditto -c -k --sequesterRsrc --keepParent AltDeckCompressed AltDeckCompressed.zip
rm -rf AltDeckCompressed

echo "Build complete: AltDeckCompressed.zip"
