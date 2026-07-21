#!/usr/bin/env bash
# Builds the launcher with updater artifacts signed using the local dev key.
# The private key never leaves this machine; it's only read into an env var
# for this process. For CI, the equivalent is the TAURI_SIGNING_PRIVATE_KEY
# GitHub Actions secret (see .github/workflows/release.yml).
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KEY_FILE="$ROOT_DIR/.tauri-keys/launcher.key"

if [ ! -f "$KEY_FILE" ]; then
  echo "Signing key not found at $KEY_FILE" >&2
  echo "Generate one with: pnpm tauri signer generate -w .tauri-keys/launcher.key" >&2
  exit 1
fi

export TAURI_SIGNING_PRIVATE_KEY_PATH="$KEY_FILE"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"

cd "$ROOT_DIR"
pnpm tauri build "$@"
