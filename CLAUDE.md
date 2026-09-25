# ContextTrace contributor notes

Project overview, user installation guidance, compatibility policy and release
status are documented in [README.md](README.md) and [docs/](docs/README.md).
Keep this file focused on repeatable development and Windows verification; do
not put credentials, private corpus details or machine-specific runner paths in
tracked files.

## Verify changes

Run from the repository root unless a command changes directory explicitly:

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm ci --prefix crates/ct-ui
npm test --prefix crates/ct-ui
npm run build --prefix crates/ct-ui
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/check-fixture-manifest.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/test-fixture-manifest.ps1
```

Verification snapshot (2026-09-25): the commands above passed, including all
Rust workspace tests, 148 frontend tests, the frontend production build and
the 8-fixture compatibility manifest checks. The Windows Tauri production
build also passed with `tauri build --no-bundle --ci -- --locked`. Full results
and remaining release gates are in [MVP status](docs/MVP-STATUS.md). These
source/build checks do not establish clean-host install, uninstall or updater
acceptance for 0.1.2; older-version installer checks are not a substitute.

The fixture catalog is the compatibility contract for persisted Codex CLI and
Claude Code session JSONL. New producer shapes need redacted fixtures,
catalog entries and semantic regression assertions. Do not claim support for
every producer version or for stdout/app-server streams unless those surfaces
are separately captured and tested.

On Windows, the native MSVC linker must be available. If PowerShell cannot
resolve `cargo` although Rust is installed, add the current user's Cargo bin
directory to this process's `PATH` (commonly `$env:USERPROFILE\.cargo\bin`),
then retry; do not change repository configuration to encode a developer's
local path. A restricted Windows shell may also deny the child process Vite
uses to load its config (`spawn EPERM`); retry the frontend tests in the normal
approved development process before treating that as a source failure. The
Tauri build must run with `crates/ct-ui` as its working
directory because it resolves its configuration there. A production desktop
binary requires Tauri's production build path, not a plain `cargo build`.

In Windows PowerShell 5.1, `$ErrorActionPreference = 'Stop'` does not reliably
turn a native executable's non-zero exit into a terminating error. Check
`$LASTEXITCODE` after native commands, and prefer one native command per CI
step or run assertion scripts as child processes with an explicit exit code.

## Windows install and data lifecycle

The NSIS installer is per-user and does not require elevation. Upgrading means
running the newer installer over the existing installation. Uninstall removes
the app and shortcuts but intentionally preserves `%LOCALAPPDATA%\ContextTrace-archive`:
it is a sibling of the application install directory and can contain the only
remaining copies of archived sessions. Tell users explicitly that archives
must be deleted separately if they want them removed. Portable desktop builds
still require Microsoft Edge WebView2 Runtime.

## Release safety

The updater signing key is a release secret, never a committed file or
command-line argument. Woodpecker injects it into the trusted tag packaging
step; the script removes it before child build processes, restores it only for
Tauri's NSIS bundling/signing command, then clears it. This is not isolation
from the runner host itself. Keep tag builds restricted to trusted maintainers.
Never run untrusted pull-request code on a self-hosted Windows runner that has
access to credentials or the developer's machine.

The release is not complete until the version-matched installer, updater
signature/feed, portable desktop archive, CLI archive and checksums have been
published, downloaded and checked, and installation/upgrade/uninstall
preservation have been verified on a separate clean Windows host. A local
build or a green compile alone is not that acceptance evidence.
