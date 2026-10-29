#!/bin/bash
# Trusted native preparation; Windows tag pipeline signs and publishes.
set -euo pipefail
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || { echo 'Linux x86_64 host required.' >&2; exit 1; }
[[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]] || { echo 'Updater secrets must remain on Windows.' >&2; exit 1; }
[[ $# -eq 3 ]] || { echo 'Usage: bash scripts/release/prepare-linux.sh v<version> <40-character commit> <new absolute output directory>' >&2; exit 1; }
tag=$1; commit=$2; out=$3
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ && "$commit" =~ ^[0-9a-f]{40}$ && "$out" == /* && ! -e "$out" && -d "$(dirname "$out")" ]] || { echo 'Invalid tag, commit or output directory.' >&2; exit 1; }
for tool in git node npm cargo pkg-config file dpkg-deb sha256sum tar; do command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }; done
for package in webkit2gtk-4.1 gtk+-3.0 libsoup-3.0; do pkg-config --exists "$package" || { echo "Missing development package: $package" >&2; exit 1; }; done
cd "$(dirname "$0")/../.."; root=$(pwd -P)
out_parent=$(cd "$(dirname "$out")" && pwd -P)
case "$out_parent/" in "$root/"*) echo 'Output must be outside the checkout.' >&2; exit 1 ;; esac
out="$out_parent/$(basename "$out")"
[[ "$(git rev-parse HEAD)" == "$commit" && -z "$(git status --porcelain --untracked-files=all)" ]] || { echo 'Checkout must be clean and match commit.' >&2; exit 1; }
if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then [[ "$(git rev-parse "refs/tags/$tag^{commit}")" == "$commit" ]] || { echo 'Tag points to another commit.' >&2; exit 1; }; fi
version=$(node -p "require('./crates/ct-ui/src-tauri/tauri.conf.json').version")
[[ "$tag" == "v$version" && "$(node -p "require('./crates/ct-ui/package.json').version")" == "$version" ]] || { echo 'Tag and desktop versions differ.' >&2; exit 1; }
workspace_version=$(cargo metadata --no-deps --format-version 1 --locked | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const p=JSON.parse(s).packages;if(!p.length||p.some(x=>x.version!==p[0].version))process.exit(1);console.log(p[0].version)})")
[[ "$workspace_version" == "$version" ]] || { echo 'Workspace version differs.' >&2; exit 1; }
mkdir -p target/linux-release-prep
work=$(mktemp -d "$root/target/linux-release-prep/x64.XXXXXXXX")
export CARGO_TARGET_DIR="$work/build" CI=true APPIMAGE_EXTRACT_AND_RUN=1
npm ci --prefix crates/ct-ui
cargo build --release --locked -p ct-cli
(cd crates/ct-ui && npm run tauri -- build --ci --bundles deb --config src-tauri/tauri.linux.conf.json -- --locked)
shopt -s nullglob
debs=("$CARGO_TARGET_DIR"/release/bundle/deb/*.deb)
[[ ${#debs[@]} -eq 1 && "$(dpkg-deb -f "${debs[0]}" Architecture)" == amd64 ]] || { echo 'Expected one amd64 deb.' >&2; exit 1; }
# The updater is enabled only in the AppImage build; signing happens on Windows.
(cd crates/ct-ui && npm run tauri -- build --ci --features linux-updater --config src-tauri/tauri.linux-release.conf.json -- --locked)
images=("$CARGO_TARGET_DIR"/release/bundle/appimage/*.AppImage)
[[ ${#images[@]} -eq 1 ]] || { echo 'Expected one AppImage.' >&2; exit 1; }
cli="$CARGO_TARGET_DIR/release/ct"
file "$cli" | grep -q 'x86-64'; file "${images[0]}" | grep -q 'x86-64'
"$cli" --help >/dev/null
mkdir "$work/artifacts" "$work/cli"
prefix="ContextTrace-$version-linux-x64"
cp "${debs[0]}" "$work/artifacts/$prefix.deb"
cp "${images[0]}" "$work/artifacts/$prefix.AppImage"
cp "$cli" LICENSE "$work/cli/"
tar -czf "$work/artifacts/$prefix-cli.tar.gz" -C "$work/cli" ct LICENSE
node - "$work/artifacts" "$tag" "$commit" <<'NODE'
const fs=require('fs'), path=require('path'), crypto=require('crypto');
const [dir,tag,commit]=process.argv.slice(2), files={};
for (const name of fs.readdirSync(dir).sort()) files[name]=crypto.createHash('sha256').update(fs.readFileSync(path.join(dir,name))).digest('hex');
fs.writeFileSync(path.join(dir,'evidence.json'),JSON.stringify({schema:1,tag,commit,arch:'x64',files},null,2)+'\n');
NODE
mkdir "$out"
cp "$work/artifacts/"* "$out/"
echo "Prepared Linux x64 artifacts at $out; transfer using the trusted release channel."
