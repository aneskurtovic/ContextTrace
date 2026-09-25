# Windows release procedure

The release target is Windows x64. A version-matched `v<version>` tag is
packaged by the trusted Windows release workflow. The workflow validates that
the Git tag, desktop version and workspace package versions agree, then stages
these six files for the release:

- `ContextTrace-<version>-windows-x64-setup.exe` — per-user NSIS installer;
- `ContextTrace-<version>-windows-x64-setup.exe.sig` — updater signature;
- `latest.json` — updater feed metadata;
- `ContextTrace-<version>-windows-x64-portable.zip` — desktop executable,
  license and portable-use notes;
- `ContextTrace-<version>-windows-x64-cli.zip` — `ct.exe` and license;
- `SHA256SUMS.txt` — checksums for the downloadable files.

The tag workflow stages artifacts on the trusted Windows agent. The manual
Woodpecker workflow `release-upload.yaml` uploads the staged files to a GitHub
draft using the repository secret `GITHUB_RELEASE_TOKEN`. It does not rebuild
the package or publish it. After acceptance on a separate clean Windows host,
publish the draft as stable (not prerelease), because the updater checks the
stable `releases/latest/download/latest.json` feed. Keep the feed's installer
URL pinned to its versioned release asset.

For the current `v0.1.2` candidate, add a fine-grained repository token with
Contents: write permission as the protected Woodpecker secret
`GITHUB_RELEASE_TOKEN`, restricted to the `manual` event, then manually run the
`release-upload` workflow on `main`. It verifies the staged filenames,
checksums and updater manifest before creating or resuming the draft. The
script fails rather than replacing a published release or a mismatched asset.

## Before tagging

1. Confirm all source checks pass and the release version is consistent in
   `Cargo.toml`, `crates/ct-ui/package.json` and
   `crates/ct-ui/src-tauri/tauri.conf.json` (plus their lockfiles).
2. Confirm the fixture manifest and compatibility regression validator pass.
3. Confirm the updater signing key is configured as a protected tag-only
   release secret. Never commit it or expose it to build/test steps.
4. Confirm only trusted maintainers can create release tags. Never run
   untrusted pull-request code on a self-hosted Windows runner with access to
   the signing secret.
5. Ensure the release workflow is present on the target branch before pushing
   the version-matched tag.

The release script builds the production desktop binary before restoring the
updater private key to the process environment for NSIS bundling. The key is
cleared immediately afterwards. The upload workflow reads only the staged
assets and uses a separate token with repository Contents write permission;
it never receives the updater private key.

## Verify and publish

After packaging, check all six files exist and validate each downloaded file
against `SHA256SUMS.txt`:

```powershell
Get-Content .\SHA256SUMS.txt
Get-FileHash .\ContextTrace-<version>-windows-x64-setup.exe -Algorithm SHA256
Get-FileHash .\ContextTrace-<version>-windows-x64-setup.exe.sig -Algorithm SHA256
Get-FileHash .\latest.json -Algorithm SHA256
Get-FileHash .\ContextTrace-<version>-windows-x64-portable.zip -Algorithm SHA256
Get-FileHash .\ContextTrace-<version>-windows-x64-cli.zip -Algorithm SHA256
```

Validate the installer signature and manifest signature correspondence,
downloaded updater feed, version-pinned installer URL, portable desktop
startup, and `ct.exe --help`. On a separate clean Windows host:

1. install without administrator elevation and launch from the Start menu;
2. confirm discovery and inspection for both supported agent log formats;
3. run the newer installer over the previous version and verify the app still
   launches;
4. uninstall and confirm app files/shortcuts are removed while
   `%LOCALAPPDATA%\ContextTrace-archive` remains;
5. run the portable desktop app and CLI archive, noting that the desktop
   requires Microsoft Edge WebView2 Runtime;
6. verify the signed updater offer and user-confirmed update flow, then record
   any crash, stale-data or format-drift evidence.

Publish only after the separate-host downloaded-artifact checks pass. The
unsigned development installer can trigger SmartScreen; do not describe a
release as signed unless Windows reports a valid Authenticode signature and
the updater's Tauri signature also verifies.
