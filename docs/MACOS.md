# macOS port

The macOS port is in development. Native compilation, launch and installation
have not yet been verified. Windows remains the published release target.

## Build on a Mac

Install Xcode command line tools, Rust via rustup with rustfmt/clippy, Node.js
22.13 or newer, and PowerShell (`pwsh`) for the fixture validators. Ensure
there is enough free disk space for a fresh Rust/Tauri build. From a checkout, run:

```sh
bash scripts/ci/check-macos.sh
```

This runs workspace tests and lint, frontend tests, both fixture manifest
validators, and production CLI and desktop builds. Tauri automatically merges
`tauri.macos.conf.json`. Each run uses a fresh directory under
`target/macos-handoffs/` and prints its `artifacts` directory on success. It
contains a DMG, CLI tarball, SHA-256 checksums and build information including
the commit and whether the worktree was dirty. The script checks the binary
architectures, runs CLI help and verifies the DMG image. Save the complete
console log separately, including on failure. To verify transferred files, run
`shasum -a 256 -c SHA256SUMS.txt` in the artifacts directory.

Builds use the host architecture: Apple Silicon on an Apple Silicon Mac,
Intel on an Intel Mac. This is not a universal binary. The script installs
the matching Rust target if needed. A fresh target directory avoids stale
artifacts but uses substantial disk space; remove a completed run directory
only after saving its evidence and artifacts.
The configured deployment floor is macOS 10.15; that is not evidence of
runtime compatibility with that version.

The opt-in Woodpecker `macos` workflow is configured to run the same command on a trusted
`darwin/arm64`, `backend=local` agent when manually triggered on `main` with
`MACOS_VALIDATE=true`. No such runner or artifact transfer is established by
this repository; provision and secure them before triggering it. The local
backend workspace is ephemeral, so configure a trusted artifact collection
mechanism before relying on its handoff files. It does not run pull-request
code, package version tags, or publish assets.

## Data and installation

Fresh source builds use
`~/Library/Application Support/ContextTrace/archive` for archives and
notification state. `CONTEXTTRACE_ARCHIVE` overrides this. If only the legacy
archive at `$XDG_DATA_HOME/contexttrace/archive` or, when XDG is unset,
`~/.local/share/contexttrace/archive` exists, it remains in use without moving
data. If both native and legacy archives exist, the native location takes
precedence; the legacy archive is left untouched.
Deleting the application must leave these directories intact.

Session discovery uses the existing Codex and Claude Code adapters and home
directory conventions. Command-K opens search.

## Remaining release gates

- Run native builds and fixture checks on Apple Silicon and Intel Macs.
- Verify discovery, inspection, archive/export, relaunch, and archive
  preservation on a separate clean Mac; record OS and architecture.
- Verify native OS notification delivery from the `.app`, including disabled
  notifications and Focus mode. The native backend reports submission errors;
  successful submission does not prove a banner was displayed. Bare Cargo
  executables cannot send notifications under the application's bundle identity.
- Validate macOS updates end to end. Development builds do not enable the
  updater; the separate release preparation enables it explicitly.
- Provision Developer ID signing, notarization, trusted native build and transfer
  infrastructure, then validate the optional release orchestration and feed.
  Record native build evidence and separate clean-host Gatekeeper, installer and
  updater acceptance independently of Woodpecker results.

Local checks or configuration files alone do not satisfy these release gates.

## Preparing signed release inputs

This path is for trusted native build infrastructure, not the development
handoff above. It has not yet been exercised on either architecture. Before a
version tag is published, prepare from the clean commit intended for that tag
on each native architecture. A Developer ID Application identity must be
installed in the signing keychain. Set `APPLE_SIGNING_IDENTITY`,
`APPLE_API_KEY`, `APPLE_API_ISSUER` and absolute `APPLE_API_KEY_PATH` for Tauri's app
notarization, and `CONTEXTTRACE_NOTARY_PROFILE` to a pre-provisioned
`xcrun notarytool` keychain profile for DMG and CLI notarization. Keep these
credentials restricted to trusted build hosts. Never provide the Tauri updater
private key to a Mac build.

```sh
bash scripts/release/prepare-macos.sh v<version> <40-character-commit> /absolute/new/output-directory
```

The output directory must not exist and must be outside the checkout. The
script requires the checkout and any existing tag to match the supplied commit
and checks all package versions. It signs and notarizes the app, DMG and CLI,
checks the app and DMG staples, Gatekeeper assessment and architecture, mounts
the DMG to inspect its app, and verifies the unpacked updater app. The `macos-updater` feature is enabled only
for this release preparation; updater eligibility additionally requires the
app to be directly in `/Applications` or `~/Applications`. The three architecture-labelled artifacts
and `evidence.json` contain no Tauri updater signature; signing that archive is
the Windows tag pipeline's responsibility. A hash manifest does not establish
provenance by itself: transfer each architecture directory over a trusted,
access-controlled channel. Retain the native console log and the private build
directory printed by the script, including its notarytool receipts and signing
details, as separate evidence; do not add them to the intake directory. Do not
publish these inputs directly or treat a local run as Woodpecker release
evidence. The keychain profile must refer to credentials for the same Apple
team as the identity. Clean-host installer and updater acceptance remain
separate gates.

## First Mac test handoff

1. Record `sw_vers` and `uname -m`, then run the validation script above and
   retain its output. A failure is useful evidence; do not skip failed gates.
2. Open the generated DMG, copy ContextTrace into Applications, and launch it
   from Finder. Record any Gatekeeper or launch error exactly. Development
   bundles are not yet a notarized public release.
3. Confirm Codex and Claude sessions appear, open one of each, inspect turns,
   search with Command-K, and export a session. Test paths containing spaces.
4. Archive a disposable session, restart the app, and verify the archived
   session remains readable. Check the archive location shown by the app.
5. Enable notifications and send a test. Check both the in-app feed and macOS
   Notification Center, then repeat with notifications disabled in System
   Settings. Report submission status separately from visible delivery.
6. Quit, reopen, and launch a second instance. Confirm only one monitor runs
   and the existing window receives focus.
7. Remove the app and confirm the archive is preserved, then reinstall and
   confirm the archived session remains available. Do not use irreplaceable
   sessions for this first acceptance pass.
8. Extract the CLI tarball, run `./ct --help` and exercise discovery/inspection
   from Terminal.

Automatic updating is not part of this initial development bundle. Testing it
requires provisioned release signing and trusted transfer, native preparation,
the optional release feed, and a separate clean-host acceptance pass.
