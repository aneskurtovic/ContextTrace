#!/bin/bash
# Trusted native preparation before a tag; final signing and publishing are on Windows.
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo 'Native macOS preparation requires a trusted Mac.' >&2; exit 1
fi
[[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]] || { echo 'Updater signing secrets must remain on Windows.' >&2; exit 1; }
if [[ $# -ne 3 ]]; then
  echo 'Usage: bash scripts/release/prepare-macos.sh v<version> <40-character commit> <new output directory>' >&2; exit 1
fi
tag=$1; expected_commit=$2; out=$3
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ && "$expected_commit" =~ ^[0-9a-f]{40}$ ]] || { echo 'Invalid tag or commit.' >&2; exit 1; }
[[ "$out" == /* ]] || { echo 'Output directory must be an absolute path.' >&2; exit 1; }
for tool in git node npm cargo rustup xcrun codesign spctl lipo hdiutil ditto shasum tar cmp; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
xcrun --find clang >/dev/null
case "$(uname -m)" in
  arm64) arch=arm64; target=aarch64-apple-darwin ;;
  x86_64) arch=x64; target=x86_64-apple-darwin ;;
  *) echo 'Unsupported host architecture.' >&2; exit 1 ;;
esac
[[ ! -e "$out" ]] || { echo 'Output directory must not already exist.' >&2; exit 1; }
[[ -d "$(dirname "$out")" ]] || { echo 'Output parent directory must exist.' >&2; exit 1; }

# Keep notarization credentials out of the build subprocess environment.
: "${APPLE_SIGNING_IDENTITY:?Set APPLE_SIGNING_IDENTITY to a Developer ID Application identity}"
: "${APPLE_API_KEY:?Set APPLE_API_KEY}"
: "${APPLE_API_ISSUER:?Set APPLE_API_ISSUER}"
: "${APPLE_API_KEY_PATH:?Set APPLE_API_KEY_PATH}"
: "${CONTEXTTRACE_NOTARY_PROFILE:?Set the notarytool keychain profile name}"
[[ "$APPLE_SIGNING_IDENTITY" == 'Developer ID Application:'* && "$APPLE_API_KEY_PATH" == /* && -f "$APPLE_API_KEY_PATH" ]] || { echo 'Developer ID identity and an absolute API key file path are required.' >&2; exit 1; }
identity=$APPLE_SIGNING_IDENTITY; api_key=$APPLE_API_KEY; api_issuer=$APPLE_API_ISSUER; api_path=$APPLE_API_KEY_PATH; notary_profile=$CONTEXTTRACE_NOTARY_PROFILE
unset APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH CONTEXTTRACE_NOTARY_PROFILE
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID

cd "$(dirname "$0")/../.."
root=$(pwd -P)
out_parent=$(cd "$(dirname "$out")" && pwd -P)
case "$out_parent/" in "$root/"*) echo 'Output must be outside the checkout.' >&2; exit 1 ;; esac
out="$out_parent/$(basename "$out")"
[[ "$(git rev-parse HEAD)" == "$expected_commit" ]] || { echo 'Checkout does not match requested commit.' >&2; exit 1; }
[[ -z "$(git status --porcelain --untracked-files=all)" ]] || { echo 'Checkout must be clean.' >&2; exit 1; }
if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
  [[ "$(git rev-parse "refs/tags/$tag^{commit}")" == "$expected_commit" ]] || { echo 'Existing tag points to a different commit.' >&2; exit 1; }
fi
version=$(node -p "require('./crates/ct-ui/src-tauri/tauri.conf.json').version")
[[ "$tag" == "v$version" && "$(node -p "require('./crates/ct-ui/package.json').version")" == "$version" ]] || { echo 'Tag and desktop versions differ.' >&2; exit 1; }
workspace_version=$(cargo metadata --no-deps --format-version 1 --locked | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const p=JSON.parse(s).packages;if(!p.length || p.some(x=>x.version!==p[0].version))process.exit(1);console.log(p[0].version)})")
[[ "$workspace_version" == "$version" ]] || { echo 'Workspace version differs.' >&2; exit 1; }

mkdir -p "$root/target/macos-release-prep"
work=$(mktemp -d "$root/target/macos-release-prep/${arch}.XXXXXXXX")
echo "Native build evidence and notarization receipts: $work"
export CARGO_TARGET_DIR="$work/build" CI=true
rustup target add "$target"
npm ci --prefix crates/ct-ui
cargo build --release --locked --target "$target" -p ct-cli
(cd crates/ct-ui && npm run tauri -- build --no-bundle --ci --target "$target" --features macos-updater --config src-tauri/tauri.macos-release.conf.json -- --locked)
(
  export APPLE_SIGNING_IDENTITY="$identity" APPLE_API_KEY="$api_key" APPLE_API_ISSUER="$api_issuer" APPLE_API_KEY_PATH="$api_path"
  cd crates/ct-ui
  npm run tauri -- bundle --ci --target "$target" --features macos-updater --config src-tauri/tauri.macos-release.conf.json
)

build="$CARGO_TARGET_DIR/$target/release"
app="$build/bundle/macos/ContextTrace.app"; cli="$build/ct"
[[ -d "$app" && -x "$cli" ]] || { echo 'App or CLI missing.' >&2; exit 1; }
app_executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
[[ "$app_executable" != */* && -x "$app/Contents/MacOS/$app_executable" ]] || { echo 'Invalid app executable.' >&2; exit 1; }
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")" == "$version" ]] || { echo 'App bundle version differs.' >&2; exit 1; }
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")" == 'dev.contexttrace.desktop' ]] || { echo 'Unexpected app bundle identifier.' >&2; exit 1; }
lipo -verify_arch "$(uname -m)" "$app/Contents/MacOS/$app_executable"
lipo -verify_arch "$(uname -m)" "$cli"
codesign --verify --deep --strict --verbose=2 "$app"
codesign -d --verbose=4 "$app" > "$work/app-signature.txt" 2>&1
xcrun stapler validate "$app"
spctl --assess --type execute --verbose "$app"

shopt -s nullglob
dmgs=("$build"/bundle/dmg/*.dmg)
[[ ${#dmgs[@]} -eq 1 ]] || { echo 'Expected exactly one DMG.' >&2; exit 1; }
codesign --force --timestamp --sign "$identity" "${dmgs[0]}"
xcrun notarytool submit "${dmgs[0]}" --keychain-profile "$notary_profile" --wait --output-format json > "$work/dmg-notary.json"
node -e "if(require(process.argv[1]).status!=='Accepted')process.exit(1)" "$work/dmg-notary.json"
xcrun stapler staple "${dmgs[0]}"
xcrun stapler validate "${dmgs[0]}"
codesign --verify --strict --verbose=2 "${dmgs[0]}"
codesign -d --verbose=4 "${dmgs[0]}" > "$work/dmg-signature.txt" 2>&1
hdiutil verify "${dmgs[0]}"
mkdir "$work/mount"
hdiutil attach "${dmgs[0]}" -readonly -nobrowse -mountpoint "$work/mount" >/dev/null
trap 'hdiutil detach "$work/mount" >/dev/null 2>&1 || true' EXIT
mounted_app="$work/mount/ContextTrace.app"
[[ -d "$mounted_app" ]] || { echo 'DMG does not contain the expected app.' >&2; exit 1; }
codesign --verify --deep --strict --verbose=2 "$mounted_app"
xcrun stapler validate "$mounted_app"
cmp "$app/Contents/MacOS/$app_executable" "$mounted_app/Contents/MacOS/$app_executable"
hdiutil detach "$work/mount" >/dev/null
trap - EXIT

codesign --force --options runtime --timestamp --sign "$identity" "$cli"
codesign --verify --strict --verbose=2 "$cli"
codesign -d --verbose=4 "$cli" > "$work/cli-signature.txt" 2>&1
ditto -c -k --keepParent "$cli" "$work/cli-notary.zip"
xcrun notarytool submit "$work/cli-notary.zip" --keychain-profile "$notary_profile" --wait --output-format json > "$work/cli-notary.json"
node -e "if(require(process.argv[1]).status!=='Accepted')process.exit(1)" "$work/cli-notary.json"
spctl --assess --type execute --verbose "$cli"
"$cli" --help >/dev/null

# Confirm the updater archive contains the same verifiably signed, stapled app.
prefix="ContextTrace-$version-macos-$arch"
mkdir -p "$work/artifacts" "$work/cli" "$work/verify"
cp "${dmgs[0]}" "$work/artifacts/$prefix.dmg"
cp "$cli" "$root/LICENSE" "$work/cli/"
tar -czf "$work/artifacts/$prefix-cli.tar.gz" -C "$work/cli" ct LICENSE
COPYFILE_DISABLE=1 tar -czf "$work/artifacts/$prefix-updater.app.tar.gz" -C "$build/bundle/macos" ContextTrace.app
cargo run --release --locked -p ct-ui --example verify-macos-updater-archive -- "$work/artifacts/$prefix-updater.app.tar.gz" "$work/verify/ContextTrace.app"
codesign --verify --deep --strict --verbose=2 "$work/verify/ContextTrace.app"
xcrun stapler validate "$work/verify/ContextTrace.app"
spctl --assess --type execute --verbose "$work/verify/ContextTrace.app"
node - "$work/artifacts" "$tag" "$expected_commit" "$arch" <<'NODE'
const fs = require('fs'), path = require('path'), crypto = require('crypto');
const [dir, tag, commit, arch] = process.argv.slice(2);
const files = {};
for (const name of fs.readdirSync(dir).sort()) files[name] = crypto.createHash('sha256').update(fs.readFileSync(path.join(dir, name))).digest('hex');
fs.writeFileSync(path.join(dir, 'evidence.json'), JSON.stringify({schema: 1, tag, commit, arch, files}, null, 2) + '\n');
NODE
# The output must be new and outside the checkout for an unambiguous handoff.
mkdir "$out"
cp "$work/artifacts/"* "$out/"
echo "Prepared $arch artifacts at $out; transfer using the trusted release channel."
