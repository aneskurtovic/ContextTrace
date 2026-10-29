#!/bin/bash
# Native development validation and handoff; not release evidence.
set -euo pipefail
[[ "$(uname -s)" == Linux && "$(uname -m)" == x86_64 ]] || { echo 'Linux x86_64 required.' >&2; exit 1; }
for tool in node npm cargo rustup pkg-config dpkg-deb file sha256sum tar xvfb-run dbus-run-session tauri-driver WebKitWebDriver; do
  command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done
for package in webkit2gtk-4.1 gtk+-3.0 libsoup-3.0; do pkg-config --exists "$package" || { echo "Missing development package: $package" >&2; exit 1; }; done
cd "$(dirname "$0")/../.."
root="$PWD"
version=$(node -p "require('./crates/ct-ui/src-tauri/tauri.conf.json').version")
[[ "$version" == "$(node -p "require('./crates/ct-ui/package.json').version")" ]] || { echo 'Desktop versions differ.' >&2; exit 1; }
workspace_version=$(cargo metadata --no-deps --format-version 1 --locked | node -e "let s='';process.stdin.on('data',c=>s+=c);process.stdin.on('end',()=>{const p=JSON.parse(s).packages;if(!p.length||p.some(x=>x.version!==p[0].version))process.exit(1);console.log(p[0].version)})")
[[ "$version" == "$workspace_version" ]] || { echo 'Workspace version differs.' >&2; exit 1; }
mkdir -p target/linux-handoffs
work=$(mktemp -d "$root/target/linux-handoffs/x64.XXXXXXXX")
export CARGO_TARGET_DIR="$work/build" CI=true APPIMAGE_EXTRACT_AND_RUN=1
npm ci --prefix crates/ct-ui
cargo fmt --all -- --check
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p ct-ui --release --features linux-updater --locked -- -D warnings
npm test --prefix crates/ct-ui
cargo build --release --locked -p ct-cli
(cd crates/ct-ui && npm run tauri -- build --ci --config src-tauri/tauri.linux.conf.json -- --locked)
# Use a disposable home, explicit roots, and a real WebKitWebDriver session.
# WebKit refuses its sandbox as root in containers; disable it only for this
# isolated CI smoke, never in the shipped application or a normal desktop.
smoke_home="$work/smoke-home"
mkdir -p "$smoke_home" "$work/smoke-config" "$work/smoke-data" "$work/smoke-cache" "$work/smoke-runtime" "$work/smoke-codex/sessions" "$work/smoke-claude" "$work/smoke-archive"
chmod 700 "$work/smoke-runtime"
smoke_env=("HOME=$smoke_home" "XDG_CONFIG_HOME=$work/smoke-config" "XDG_DATA_HOME=$work/smoke-data" "XDG_CACHE_HOME=$work/smoke-cache" "XDG_RUNTIME_DIR=$work/smoke-runtime" "CODEX_HOME=$work/smoke-codex" "CLAUDE_CONFIG_DIR=$work/smoke-claude" "CONTEXTTRACE_ARCHIVE=$work/smoke-archive")
if [[ "$(id -u)" == 0 ]]; then smoke_env+=("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1"); fi
env "${smoke_env[@]}" dbus-run-session -- xvfb-run -a -s "-screen 0 1920x1080x24" node scripts/ci/smoke-linux-webdriver.mjs "$CARGO_TARGET_DIR/release/context-trace" "$work/smoke-archive" "$work/webdriver.png"
cli="$CARGO_TARGET_DIR/release/ct"
file "$cli" | grep -q 'x86-64' || { echo 'Wrong CLI architecture.' >&2; exit 1; }
"$cli" --help >/dev/null
shopt -s nullglob
debs=("$CARGO_TARGET_DIR"/release/bundle/deb/*.deb)
images=("$CARGO_TARGET_DIR"/release/bundle/appimage/*.AppImage)
[[ ${#debs[@]} -eq 1 && ${#images[@]} -eq 1 ]] || { echo 'Expected exactly one deb and AppImage.' >&2; exit 1; }
[[ "$(dpkg-deb -f "${debs[0]}" Architecture)" == amd64 ]] || { echo 'Wrong deb architecture.' >&2; exit 1; }
file "${images[0]}" | grep -q 'x86-64' || { echo 'Wrong AppImage architecture.' >&2; exit 1; }
# Inspect the real package metadata. Tauri names the file after productName.
mkdir "$work/deb-extract" "$work/image-extract"
dpkg-deb -x "${debs[0]}" "$work/deb-extract"
(cd "$work/image-extract" && "${images[0]}" --appimage-extract >/dev/null)
for desktop in "$work/deb-extract/usr/share/applications/ContextTrace.desktop" "$work/image-extract/squashfs-root/usr/share/applications/ContextTrace.desktop"; do
  [[ -f "$desktop" ]] || { echo "Missing packaged desktop entry: $desktop" >&2; exit 1; }
  grep -Eq '^Exec=context-trace %u$' "$desktop" || { echo "Invalid desktop Exec in $desktop" >&2; exit 1; }
  grep -Eq '^MimeType=([^;]*;)*x-scheme-handler/contexttrace(;|$)' "$desktop" || { echo "Missing contexttrace URI handler in $desktop" >&2; exit 1; }
done

out="$work/artifacts"; mkdir -p "$out" "$work/cli"
prefix="ContextTrace-$version-linux-x64"
cp "${debs[0]}" "$out/$prefix.deb"
cp "${images[0]}" "$out/$prefix.AppImage"
cp "$cli" LICENSE "$work/cli/"
tar -czf "$out/$prefix-cli.tar.gz" -C "$work/cli" ct LICENSE
(cd "$out" && sha256sum ./*.deb ./*.AppImage ./*.tar.gz > SHA256SUMS.txt)
{
  printf 'Development handoff; not release evidence.\nVersion: %s\nArchitecture: x64\nCommit: %s\n' "$version" "$(git rev-parse HEAD)"
  if [[ -n "$(git status --porcelain)" ]]; then echo 'Worktree: dirty'; else echo 'Worktree: clean'; fi
  rustc --version; node --version
} > "$out/BUILD-INFO.txt"
echo "Test handoff: $out"
