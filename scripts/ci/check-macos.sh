#!/bin/bash
# Native development validation and test handoff, not release packaging.
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  echo 'macOS validation must run on a Mac with Xcode command line tools.' >&2
  exit 1
fi

for tool in node npm cargo rustup pwsh xcrun lipo hdiutil; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
xcrun --find clang >/dev/null || { echo 'Xcode command line tools are required.' >&2; exit 1; }

cd "$(dirname "$0")/../.."
root="$PWD"
case "$(uname -m)" in
  arm64) arch=arm64; rust_target=aarch64-apple-darwin ;;
  x86_64) arch=x64; rust_target=x86_64-apple-darwin ;;
  *) echo 'Unsupported macOS host architecture.' >&2; exit 1 ;;
esac

# Fresh output ensures a failed build cannot hand off a stale DMG.
mkdir -p "$root/target/macos-handoffs"
run_dir=$(mktemp -d "$root/target/macos-handoffs/${arch}.XXXXXXXX")
export CARGO_TARGET_DIR="$run_dir/build"
export CI=true

version=$(node -p "require('./crates/ct-ui/src-tauri/tauri.conf.json').version")
package_version=$(node -p "require('./crates/ct-ui/package.json').version")
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]] || { echo 'Invalid desktop version.' >&2; exit 1; }
[[ "$version" == "$package_version" ]] || { echo 'Desktop and package versions differ.' >&2; exit 1; }
workspace_version=$(cargo metadata --no-deps --format-version 1 --locked | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const p=JSON.parse(s).packages.find(p=>p.name==='ct-cli');if(!p)process.exit(1);console.log(p.version)})")
[[ "$version" == "$workspace_version" ]] || { echo 'Desktop and workspace versions differ.' >&2; exit 1; }

echo "Building macOS $arch ($rust_target), version $version; work directory: $run_dir"
rustup target add "$rust_target"
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm ci --prefix crates/ct-ui
npm test --prefix crates/ct-ui
pwsh -NoProfile -File scripts/ci/check-fixture-manifest.ps1
pwsh -NoProfile -File scripts/ci/test-fixture-manifest.ps1
cargo build --release --locked --target "$rust_target" -p ct-cli
(cd crates/ct-ui && npm run tauri -- build --ci --target "$rust_target" -- --locked)

build="$CARGO_TARGET_DIR/$rust_target/release"
app="$build/bundle/macos/ContextTrace.app"
cli="$build/ct"
[[ -d "$app" && -x "$cli" ]] || { echo 'Expected app or CLI is missing.' >&2; exit 1; }
app_executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
[[ "$app_executable" != */* && -x "$app/Contents/MacOS/$app_executable" ]] || { echo 'Invalid app executable.' >&2; exit 1; }
lipo -verify_arch "$(uname -m)" "$app/Contents/MacOS/$app_executable"
lipo -verify_arch "$(uname -m)" "$cli"
"$cli" --help >/dev/null

shopt -s nullglob
dmgs=("$build"/bundle/dmg/*.dmg)
[[ ${#dmgs[@]} -eq 1 ]] || { echo "Expected one DMG, found ${#dmgs[@]}." >&2; exit 1; }
hdiutil verify "${dmgs[0]}"
out="$run_dir/artifacts"
mkdir -p "$out" "$run_dir/cli"
cp "${dmgs[0]}" "$out/ContextTrace-$version-macos-$arch.dmg"
cp "$cli" "$root/LICENSE" "$run_dir/cli/"
tar -czf "$out/ContextTrace-$version-macos-$arch-cli.tar.gz" -C "$run_dir/cli" ct LICENSE
(
  cd "$out"
  shasum -a 256 ./*.dmg ./*.tar.gz > SHA256SUMS.txt
)
{
  printf 'Development test handoff; not a published release.\n'
  printf 'Version: %s\nArchitecture: %s\nRust target: %s\n' "$version" "$arch" "$rust_target"
  printf 'Commit: %s\n' "$(git rev-parse HEAD)"
  if [[ -n "$(git status --porcelain)" ]]; then printf 'Worktree: dirty (artifacts may differ from commit)\n'; else printf 'Worktree: clean\n'; fi
  sw_vers
  rustc --version
  node --version
} > "$out/BUILD-INFO.txt"
echo "Test handoff: $out"
