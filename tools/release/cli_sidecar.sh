#!/usr/bin/env bash
# Builds ukiyo-cli and puts it where src-tauri/tauri.bundle.conf.json's externalBin looks:
# src-tauri/binaries/ukiyo-cli-<target triple>. The bundler copies it into
# Ukiyo.app/Contents/MacOS/ukiyo-cli.
#
# `tauri build --config tauri.bundle.conf.json` runs this as its beforeBuildCommand, with
# TAURI_ENV_TARGET_TRIPLE and TAURI_ENV_DEBUG set. Run by hand, it builds a release binary
# for this machine.
#
# Why an overlay config and not tauri.conf.json: tauri-build copies every externalBin into
# target/<profile>/ under its short name. In a debug build that would replace the real
# target/debug/ukiyo-cli, which the CLI tests run.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
manifest="$root/src-tauri/Cargo.toml"
target_dir="${CARGO_TARGET_DIR:-$root/src-tauri/target}"
host="$(rustc -vV | sed -n 's/^host: //p')"
triple="${TAURI_ENV_TARGET_TRIPLE:-$host}"

profile=release
args=(build -p ukiyo-cli --manifest-path "$manifest")
if [ "${TAURI_ENV_DEBUG:-false}" = true ]; then
  profile=debug
else
  args+=(--release)
fi

if [ "$triple" = "$host" ]; then
  out="$target_dir/$profile/ukiyo-cli"
else
  args+=(--target "$triple")
  out="$target_dir/$triple/$profile/ukiyo-cli"
fi

cargo "${args[@]}"
mkdir -p "$root/src-tauri/binaries"
dest="$root/src-tauri/binaries/ukiyo-cli-$triple"
cp -f "$out" "$dest"
echo "cli_sidecar: $dest ($profile, $triple)"
