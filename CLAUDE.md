# ContextTrace contributor notes

Project overview, user installation guidance, compatibility policy and release
status are documented in [README.md](README.md) and [docs/](docs/README.md).
Keep this file focused on repeatable development and Windows verification; do
not put credentials, private corpus details or machine-specific runner paths in
tracked files.

## CI and release policy

Always use Woodpecker CI for continuous integration and release packaging. Never
add or run GitHub Actions jobs, use GitHub-hosted runners, or dispatch the
repository's manual GitHub release workflow. Keep validation and Windows
release builds on the configured Woodpecker runners.
Version-tag packaging, publishing, and published-asset verification belong in
the Woodpecker Windows release pipeline. Local development checks are useful,
but an interactive workstation run is not release evidence. Keep separate
clean-host installer and updater acceptance distinct from CI results.

## Verify changes

Run from the repository root unless a command changes directory explicitly:

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm ci --prefix crates/ct-ui
npm test --prefix crates/ct-ui
npm run build --prefix crates/ct-ui
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release/test-release-notes.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/check-fixture-manifest.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/test-fixture-manifest.ps1
```

Latest patch release (2026-10-08): v0.1.12 adds clickable local file targets
and fixes notification navigation and Windows toast activation. Commit
`303b502` passed all configured Woodpecker push checks in pipeline 100:
488 Rust tests passed (one ignored), 167 frontend tests passed, and Windows
desktop/CLI builds, smoke checks and Linux MSRV validation passed. Tag pipeline
101 packaged, published and verified all six assets and the latest stable pointer.
Separate clean-host installer/updater acceptance and actual Windows toast and
file-association behavior remain unverified. See
[release notes](docs/releases/v0.1.12.md) and [release procedure](docs/RELEASING.md).

Previous patch release (2026-10-08): v0.1.10 adds Claude `away_summary` and
`bridge_status` presentation notices without adding model-context weight.
Commit `78d345a` passed all configured checks in Woodpecker push pipeline 93:
481 Rust tests passed (one ignored), all 17 fixtures passed validation, and the
frontend tests/build and Windows desktop/CLI smoke checks passed. Tag pipeline
94 packaged, published and verified all six release assets. The local Claude
scan found one malformed line and no remaining warnings for the two supported
types. Local frontend tests passed with one worker and a 15-second timeout
after two default-timeout failures; configured Woodpecker frontend checks
passed normally. Separate clean-host installer/updater acceptance remains
unverified. See [compatibility](docs/FORMAT-COMPATIBILITY.md) and
[MVP status](docs/MVP-STATUS.md).

Historical follow-up verification (2026-10-08): 480 Rust tests passed (one ignored),
formatting and strict workspace Clippy passed, all 16 fixtures and validator
regressions passed, and 156 frontend tests plus the production build passed.
Separate stdout import contracts and current-version captures were added; see
[the follow-up](docs/FORMAT-FOLLOWUP-2026-10-08.md). Release commit `193150f`
passed Woodpecker push pipeline 90; tag pipeline 91 packaged and published
v0.1.9 and verified all six uploaded assets. Clean-host acceptance is unavailable
and rare Codex shapes still lack feature-matched real captures.

Historical parser-audit verification (2026-10-07): 468 Rust tests passed (one
ignored), formatting and workspace Clippy passed, 11 fixtures and validator
regressions passed, and the frontend production build passed. All 156 frontend
tests passed with a single-thread/15-second-timeout retry after Windows worker
startup and timing failures. Existing dependencies were used; no new desktop
packaging or clean-host acceptance was performed. See [HANDOFF](HANDOFF.md)
for next steps and [the audit](docs/FORMAT-AUDIT-2026-10-07.md) for exact limits.

Historical verification snapshot (2026-09-25): the commands above passed, including all
Rust workspace tests, 148 frontend tests, the frontend production build and
the 8-fixture compatibility manifest checks. The Windows Tauri production
build also passed with `tauri build --no-bundle --ci -- --locked`. Full results
and remaining release gates are in [MVP status](docs/MVP-STATUS.md). These
source/build checks do not establish clean-host install, uninstall or updater
acceptance for a new release; older-version installer checks are not a substitute.

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
