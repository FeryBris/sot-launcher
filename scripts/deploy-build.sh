#!/usr/bin/env bash
# Pulls the latest launcher source, produces a signed Linux build, and
# publishes it under releases/download/ (Ploi's launcher.sot.sk site should
# have its Web Directory set to "releases", so this ends up served at
# launcher.sot.sk/download/<filename>).
#
# This is the launcher.sot.sk side only. It does NOT register anything with
# the guild website - sot.sk's admin panel always re-uploads the actual file
# (computes sha256 server-side from real bytes, deliberately, not from a
# linked URL), so after this runs, download each file printed below from
# launcher.sot.sk and upload it through sot.sk's admin panel (Launcher >
# Releases > pick/create the version > Add Platform Artifact), matching the
# platform/file_type shown for each.
#
# Run this ON THE LAUNCHER.SOT.SK SERVER. It assumes:
#   - the repo is already cloned here and this script is run from within it
#   - .tauri-keys/launcher.key (the PRIVATE signing key) has been copied
#     here separately, e.g. `scp .tauri-keys/launcher.key
#     you@server:/path/to/launcher/.tauri-keys/` - it's gitignored on
#     purpose (see .gitignore's ".tauri-keys/*.key") and must never be
#     committed, so `git pull` alone won't bring it over.
#   - the `rpm` package is installed (provides `rpmbuild`, needed for the
#     .rpm bundle target - not part of Tauri's usual "Linux build deps"
#     list, and not installed by default on Ubuntu). `sudo apt install rpm`
#     if `tauri build` complains it's missing.
set -euo pipefail

# Ploi's deploy script runs non-interactively, which never sources
# ~/.bashrc/~/.profile - a plain rustup install (which only wires up PATH
# via those) would be invisible here even though it works fine over a
# regular SSH login. Source it directly instead of assuming it's already on
# PATH; harmless no-op if rustup was never installed at all (this file
# simply won't exist yet, `cargo`/`rustc` still fail below with a clear error).
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck source=/dev/null
  source "$HOME/.cargo/env"
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> Pulling latest source"
# --ff-only: refuses to silently create a merge commit or clobber local
# changes if this checkout has diverged - surface that instead of guessing.
git pull --ff-only

echo "==> Installing frontend dependencies"
pnpm install

# Sourced, not executed - build-signed.sh's `export TAURI_SIGNING_PRIVATE_KEY_PATH=...`
# needs to survive into THIS shell so the manual `tauri signer sign` calls
# below (for the .deb/.rpm, which the build itself doesn't sign - see
# below) can reuse it. Running it as a child process instead would have
# those exports vanish the moment it exits.
echo "==> Building (signed with .tauri-keys/launcher.key)"
source ./scripts/build-signed.sh

VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")
BUNDLE_DIR="src-tauri/target/release/bundle"
DOWNLOAD_DIR="$ROOT_DIR/releases/download"

# createUpdaterArtifacts (tauri.conf.json) produces this .tar.gz + .sig
# specifically for the updater feed - NOT the raw .AppImage sitting next to
# it, and NOT the .deb/.rpm. This is sot.sk's "linux-x86_64 / appimage-tar-gz"
# platform+file_type combo (see LauncherReleasePlatform::FILE_TYPES in the
# wotlk repo) - uploading the wrong file there would silently break the
# auto-updater for anyone already running the launcher.
UPDATER_ARTIFACT=$(find "$BUNDLE_DIR/appimage" -maxdepth 1 -name '*.AppImage.tar.gz' | head -1)
UPDATER_SIG="$UPDATER_ARTIFACT.sig"

# The raw AppImage is also a valid manual download (platform+file_type
# "linux-x86_64 / appimage") - separate from the updater bundle above, and
# Tauri doesn't sign it by default either, so it gets the same manual
# treatment as .deb/.rpm below. `-name '*.AppImage'` alone already excludes
# the .tar.gz (find's -name matches the whole basename, and
# "...AppImage.tar.gz" doesn't end in ".AppImage").
APPIMAGE_FILE=$(find "$BUNDLE_DIR/appimage" -maxdepth 1 -name '*.AppImage' | head -1)
pnpm tauri signer sign "$APPIMAGE_FILE"
APPIMAGE_SIG="$APPIMAGE_FILE.sig"

# Tauri only auto-signs the one file per platform it considers "the"
# updater artifact (above) - .deb/.rpm aren't part of that (Linux
# auto-update doesn't use them at all, they're just for manual installs),
# so they need their own explicit `tauri signer sign` call to get a
# matching .sig each.
DEB_FILE=$(find "$BUNDLE_DIR/deb" -maxdepth 1 -name '*.deb' | head -1)
pnpm tauri signer sign "$DEB_FILE"
DEB_SIG="$DEB_FILE.sig"

RPM_FILE=$(find "$BUNDLE_DIR/rpm" -maxdepth 1 -name '*.rpm' | head -1)
pnpm tauri signer sign "$RPM_FILE"
RPM_SIG="$RPM_FILE.sig"

echo "==> Publishing to $DOWNLOAD_DIR"
# Wiped and repopulated fresh each run rather than accumulated - old
# versions belong in sot.sk's release history, not scattered across every
# past launcher.sot.sk deploy.
rm -rf "$DOWNLOAD_DIR"
mkdir -p "$DOWNLOAD_DIR"
cp "$UPDATER_ARTIFACT" "$UPDATER_SIG" "$APPIMAGE_FILE" "$APPIMAGE_SIG" "$DEB_FILE" "$DEB_SIG" "$RPM_FILE" "$RPM_SIG" "$DOWNLOAD_DIR/"

BASE_URL="https://launcher.sot.sk/download"
cat <<EOF

==> Build complete: v$VERSION - published to $BASE_URL/

Now go to sot.sk's admin panel (Launcher > Releases > v$VERSION > Add
Platform Artifact) and, for each row below, download the file from its URL
and re-upload it there under the matching platform / file type (sot.sk
always re-hashes the real bytes server-side, so linking the URL directly
isn't an option):

  linux-x86_64 / appimage-tar-gz (the actual update target):
    $BASE_URL/$(basename "$UPDATER_ARTIFACT")
    $BASE_URL/$(basename "$UPDATER_SIG")

  linux-x86_64 / appimage:
    $BASE_URL/$(basename "$APPIMAGE_FILE")
    $BASE_URL/$(basename "$APPIMAGE_SIG")

  linux-x86_64 / deb:
    $BASE_URL/$(basename "$DEB_FILE")
    $BASE_URL/$(basename "$DEB_SIG")

  linux-x86_64 / rpm:
    $BASE_URL/$(basename "$RPM_FILE")
    $BASE_URL/$(basename "$RPM_SIG")
EOF
