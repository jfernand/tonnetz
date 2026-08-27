#!/usr/bin/env bash
# Downloads the Salamander Grand Piano SF2 (CC-BY 3.0, Alexander Holm; SF2
# conversion by roberto@zenvoid.org for the FreePats project -- see
# SalamanderGrandPiano-LICENSE.txt) and installs it where tonnetz-cli looks
# for it. Not committed to git -- at ~1.2GB it's far too big for this repo,
# so `assets/soundfonts/salamander/` is gitignored and this script is the
# supported way to get the file locally. Running tonnetz-cli without it
# still works; it just falls back to the bundled GeneralUser GS piano.
set -euo pipefail

URL="https://freepats.zenvoid.org/Piano/SalamanderGrandPiano/SalamanderGrandPiano-SF2-V3+20200602.tar.xz"
SHA256="15edb061d7ba60d58332f72dba8f8ce40988048cc703f935e6320f37d650e213"
SF2_NAME="SalamanderGrandPiano-V3+20200602.sf2"

DEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/salamander"
DEST_FILE="$DEST_DIR/$SF2_NAME"

if [ -f "$DEST_FILE" ]; then
    echo "Already present: $DEST_FILE"
    exit 0
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading Salamander Grand Piano (CC-BY 3.0, Alexander Holm) via FreePats..."
curl -fL --progress-bar -o "$TMP_DIR/salamander.tar.xz" "$URL"

echo "Verifying checksum..."
echo "$SHA256  $TMP_DIR/salamander.tar.xz" | sha256sum -c -

echo "Extracting..."
tar -xJf "$TMP_DIR/salamander.tar.xz" -C "$TMP_DIR"

mkdir -p "$DEST_DIR"
cp "$TMP_DIR/SalamanderGrandPiano-SF2-V3+20200602/$SF2_NAME" "$DEST_FILE"

echo "Installed to $DEST_FILE"
