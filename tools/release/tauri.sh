#!/usr/bin/env bash
# Runs the Tauri CLI for a release build. `tauri.sh build ...` adds the bundle overlay
# (src-tauri/tauri.bundle.conf.json: the ukiyo-cli sidecar) and, when UKIYO_UPDATER_PUBKEY is
# set, the updater public key in place of tauri.conf.json's placeholder. Any other subcommand
# passes through unchanged.
#
# release.sh calls it locally; .github/workflows/release.yml passes it to tauri-macos-build as
# `tauri-command`. Both build the same bundle this way. Run it from src-tauri/.
#
# The updater private key signs nothing during the build (createUpdaterArtifacts is off), so
# the build runs without it: the archive is made later, from the final bundle.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
tauri="$root/app/node_modules/.bin/tauri"

if [ "${1:-}" != build ]; then
  exec "$tauri" "$@"
fi

configs=(--config "$root/src-tauri/tauri.bundle.conf.json")
if [ -n "${UKIYO_UPDATER_PUBKEY:-}" ]; then
  configs+=(--config "$(jq -cn --arg k "$UKIYO_UPDATER_PUBKEY" '{plugins: {updater: {pubkey: $k}}}')")
else
  echo "tauri.sh: no UKIYO_UPDATER_PUBKEY: this build never checks for updates" >&2
fi
exec env -u TAURI_SIGNING_PRIVATE_KEY -u TAURI_SIGNING_PRIVATE_KEY_PASSWORD \
  "$tauri" "$@" "${configs[@]}"
