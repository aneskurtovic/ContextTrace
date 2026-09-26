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

The tag pipeline waits for the `windows`, `rust` and `frontend` validation
workflows to succeed before packaging. The Windows release workflow then signs
and stages the six files, creates a public prerelease, uploads the files, checks
all uploaded digests, and promotes the release to stable. The updater reads the
stable `releases/latest/download/latest.json` feed, so the prerelease remains
outside its update channel until all assets are verified. `latest.json` is
uploaded last, and the final promotion explicitly sets and verifies GitHub's
latest stable release pointer. A failed or interrupted publish can be retried:
matching assets are reused, missing assets are uploaded, and mismatched assets
stop the run.

Add a fine-grained repository token with Contents: write permission as the
protected Woodpecker secret `GITHUB_RELEASE_TOKEN`, restricted to the `tag`
event. The release workflow uses it only in the final publishing step; tests
and packaging do not receive it. Only trusted maintainers may create version
tags, because the tag workflow also receives the updater signing key.

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
5. Ensure the validation workflows and their release dependencies are present
   on the target branch before pushing the version-matched tag.

The release script builds the production desktop binary before restoring the
updater private key to the process environment for NSIS bundling. The key is
cleared immediately afterwards. The publishing step reads only the staged
assets and uses a separate token with repository Contents write permission;
it never receives the updater private key.

## Verify the published release

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

The tag workflow validates staged and uploaded checksums, signature and
manifest correspondence, and the version-pinned installer URL before marking
the stable release complete. After publication, validate the downloaded
updater feed and portable desktop startup. On a separate clean Windows host:

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

The Woodpecker tag pipeline publishes automatically after its validation,
packaging and uploaded-asset checks pass. The separate-host checks remain the
post-publication acceptance pass; they do not control release creation. The
unsigned development installer can trigger SmartScreen; do not describe a
release as signed unless Windows reports a valid Authenticode signature and
the updater's Tauri signature also verifies.
