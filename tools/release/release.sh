#!/usr/bin/env bash
# Builds a Ukiyo release on an Apple Silicon Mac: Ukiyo.app with the CLI inside, the arm64 zip
# the Homebrew cask installs, the updater archive with its signature, and latest.json.
#
#   tools/release/release.sh --dry-run    build and check everything, upload nothing
#   tools/release/release.sh              the same, then attach the files to the GitHub
#                                         release vX.Y.Z of noahkiss/ukiyo (gh; the tag must
#                                         exist on origin)
#
# The version is tauri.conf.json's; src-tauri/Cargo.toml must state the same one.
# Output: local/release/<version>/ (gitignored).
#
# Environment (read from 1Password into the environment, never from a file):
#   UKIYO_UPDATER_PUBKEY                the updater public key, built into the app. Without it
#                                       the app carries the placeholder and never checks for
#                                       updates (a dry run allows that).
#   TAURI_SIGNING_PRIVATE_KEY           the updater private key, which signs the archive.
#                                       Without it a dry run writes latest.json with an empty
#                                       signature.
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD  its password; empty for a key without one.
#   UKIYO_RELEASE_NOTES                 latest.json's notes (default: "Ukiyo <version>").
#
# Signing: the bundle gets tauri.conf.json's ad-hoc signature (signingIdentity "-"). A public
# release is signed with the Developer ID and notarized before the zip is made, on QuadCam's
# pattern (noahkiss/workflows: macos-sign-notarize); this script checks the signature it finds
# and says which it is.
set -euo pipefail

dry_run=false
for a in "$@"; do
  case "$a" in
    --dry-run | --no-upload) dry_run=true ;;
    -h | --help)
      sed -n '2,28p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "release: unknown argument $a" >&2
      exit 2
      ;;
  esac
done

root="$(cd "$(dirname "$0")/../.." && pwd)"
tauri_dir="$root/src-tauri"
repo="noahkiss/ukiyo"

[ "$(uname -s)" = Darwin ] && [ "$(uname -m)" = arm64 ] || { echo "release: needs an Apple Silicon Mac" >&2; exit 1; }
for tool in jq ditto codesign pnpm cargo; do
  command -v "$tool" >/dev/null || { echo "release: needs $tool" >&2; exit 1; }
done

version="$(jq -r .version "$tauri_dir/tauri.conf.json")"
cargo_version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$tauri_dir/Cargo.toml" | head -1)"
[ "$version" = "$cargo_version" ] || { echo "release: tauri.conf.json says $version, Cargo.toml says $cargo_version" >&2; exit 1; }
tag="v$version"

if ! $dry_run; then
  [ -n "${UKIYO_UPDATER_PUBKEY:-}" ] || { echo "release: UKIYO_UPDATER_PUBKEY is not set" >&2; exit 1; }
  [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] || { echo "release: TAURI_SIGNING_PRIVATE_KEY is not set" >&2; exit 1; }
  [ -z "$(git -C "$root" status --porcelain)" ] || { echo "release: the working tree has changes" >&2; exit 1; }
  command -v gh >/dev/null || { echo "release: needs gh" >&2; exit 1; }
  git -C "$root" ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null \
    || { echo "release: tag $tag is not on origin" >&2; exit 1; }
  [ "$(git -C "$root" rev-parse "$tag^{commit}")" = "$(git -C "$root" rev-parse HEAD)" ] \
    || { echo "release: HEAD is not $tag; check out the tag" >&2; exit 1; }
fi

out="$root/local/release/$version"
rm -rf "$out"
mkdir -p "$out"

# ----- build -----

configs=(--config tauri.bundle.conf.json)
if [ -n "${UKIYO_UPDATER_PUBKEY:-}" ]; then
  configs+=(--config "$(jq -cn --arg k "$UKIYO_UPDATER_PUBKEY" '{plugins: {updater: {pubkey: $k}}}')")
else
  echo "release: no UKIYO_UPDATER_PUBKEY: this build never checks for updates" >&2
fi
# The private key signs nothing during the build (createUpdaterArtifacts is off): the archive
# is made below, from the final bundle.
(
  cd "$tauri_dir"
  env -u TAURI_SIGNING_PRIVATE_KEY -u TAURI_SIGNING_PRIVATE_KEY_PASSWORD \
    "$root/app/node_modules/.bin/tauri" build --bundles app "${configs[@]}"
)

app="$tauri_dir/target/release/bundle/macos/Ukiyo.app"
[ -d "$app" ] || { echo "release: no bundle at $app" >&2; exit 1; }

# ----- check the bundle -----

plist_version="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")"
[ "$plist_version" = "$version" ] || { echo "release: the bundle says $plist_version" >&2; exit 1; }
cli="$app/Contents/MacOS/ukiyo-cli"
[ -x "$cli" ] || { echo "release: no ukiyo-cli in the bundle" >&2; exit 1; }
file "$cli" | grep -q 'Mach-O 64-bit executable arm64' || { echo "release: ukiyo-cli is not an arm64 binary: $(file "$cli")" >&2; exit 1; }
"$cli" --version
echo "schema version $("$cli" --schema-version)"
codesign --verify --deep --strict "$app"
signature="$(codesign -dv "$app" 2>&1 | sed -n 's/^Signature=//p; s/^Authority=//p' | head -1)"
echo "release: signature: ${signature:-none}"

# ----- the zip, for the cask -----

zip="$out/ukiyo-$version-arm64.zip"
# ditto keeps the signature, a stapled ticket and the bundle's symlinks.
ditto -c -k --sequesterRsrc --keepParent "$app" "$zip"
sha="$(shasum -a 256 "$zip" | cut -d' ' -f1)"

# ----- the updater archive and latest.json -----

archive="$out/ukiyo-$version-arm64.app.tar.gz"
# No AppleDouble files: the updater copies the tree as it is.
COPYFILE_DISABLE=1 tar -C "$(dirname "$app")" --no-mac-metadata --no-xattrs -czf "$archive" Ukiyo.app
check="$(mktemp -d)"
trap 'rm -rf "$check"' EXIT
tar -xzf "$archive" -C "$check"
codesign --verify --deep --strict "$check/Ukiyo.app"

sig=""
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
  "$root/app/node_modules/.bin/tauri" signer sign --app-version "$version" "$archive" </dev/null >/dev/null
  sig="$(cat "$archive.sig")"
else
  echo "release: no TAURI_SIGNING_PRIVATE_KEY: latest.json has an empty signature" >&2
fi

jq -n \
  --arg version "$version" \
  --arg notes "${UKIYO_RELEASE_NOTES:-Ukiyo $version}" \
  --arg pub_date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg signature "$sig" \
  --arg url "https://github.com/$repo/releases/download/$tag/$(basename "$archive")" \
  '{version: $version, notes: $notes, pub_date: $pub_date,
    platforms: {"darwin-aarch64": {signature: $signature, url: $url}}}' \
  >"$out/latest.json"

echo
echo "release: $version in $out"
ls -l "$out"
echo "zip sha256: $sha"

if $dry_run; then
  echo "release: dry run; nothing uploaded"
  exit 0
fi

# ----- upload -----

gh release view "$tag" -R "$repo" >/dev/null 2>&1 || gh release create "$tag" -R "$repo" --title "Ukiyo $version" --notes "${UKIYO_RELEASE_NOTES:-Ukiyo $version}"
attached="$(gh release view "$tag" -R "$repo" --json assets --jq '.assets[].name')"
# The cask's sha256 pins the published zip, and latest.json must name the archive's own
# signature: never replace or mix them. A rebuilt release gets a new version.
for f in "$zip" "$archive" "$archive.sig"; do
  if grep -qx "$(basename "$f")" <<<"$attached"; then
    echo "release: $(basename "$f") is already attached to $tag; nothing uploaded" >&2
    exit 1
  fi
done
gh release upload "$tag" -R "$repo" "$zip" "$archive" "$archive.sig"
gh release upload "$tag" -R "$repo" "$out/latest.json" --clobber
echo "release: attached to $tag. Bump the cask: version $version, sha256 $sha"
