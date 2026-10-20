#!/bin/bash

clear

LATEST_VER=$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/JadXV/AltDeck/releases/latest | sed 's|.*/tag/||')
echo "Latest version determined to be: $LATEST_VER"
echo ""

AltDeck_URL="https://github.com/JadXV/AltDeck/releases/download/$LATEST_VER/AltDeckCompressed.zip"
TMP_ZIP="/tmp/AltDeckCompressed.zip"

ARCH=$(uname -m)
if [[ "$ARCH" == "arm64" ]]; then
  ARCH_FOLDER="AltDeck-ARM64"
elif [[ "$ARCH" == "x86_64" ]]; then
  ARCH_FOLDER="AltDeck-x86_64"
else
  echo "Unsupported architecture: $ARCH"
  exit 1
fi

if [ -d "/Applications/AltDeck.app" ]; then
  echo "AltDeck is already installed."
  echo "Updating / Reinstalling AltDeck..."
  rm -rf /Applications/AltDeck.app
fi

echo "Cleaning up temporary files..."
rm -rf /tmp/AltDeck*.app /tmp/AltDeck-*.app /tmp/AltDeckCompressed

echo "Downloading AltDeck..."
curl -fsSL "$AltDeck_URL" -o "$TMP_ZIP" || {
  echo "❌ Failed to download AltDeck"
  exit 1
}

echo "Unzipping AltDeck..."
unzip -o -q "$TMP_ZIP" -d /tmp || {
  echo "❌ Failed to unzip AltDeck"
  exit 1
}

echo "Installing $ARCH_FOLDER..."
mv "/tmp/AltDeckCompressed/$ARCH_FOLDER.app" "/tmp/AltDeck.app" || {
  echo "❌ Failed to rename app folder"
  exit 1
}

mv "/tmp/AltDeck.app" "/Applications" || {
  echo "❌ Failed to move AltDeck to Applications"
  exit 1
}

xattr -rd com.apple.quarantine /Applications/AltDeck.app

rm "$TMP_ZIP"
rm -rf /tmp/AltDeckCompressed

echo ""
echo "✅ AltDeck installed successfully!"
echo "You can now find AltDeck in your Applications folder."

open -a /Applications/AltDeck.app
