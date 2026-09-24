# Windows release procedure

The preferred release path packages one Windows x64 release through the
self-hosted Woodpecker Windows agent. Push `v<version>` after the release
pipeline is present; the tag must exactly match `version` in
`crates/ct-ui/src-tauri/tauri.conf.json`; the workflow rejects mismatches.

GitHub Actions remains available as a manual fallback through **Release Windows
artifacts**, but it is no longer triggered automatically by tags. This avoids
consuming GitHub-hosted minutes for the normal release path.

It creates a draft GitHub release containing:

- `ContextTrace-<version>-windows-x64-setup.exe` — the NSIS desktop installer;
- `ContextTrace-<version>-windows-x64-portable.zip` — the desktop executable,
  license and portable-use notes; extract and run without installing;
- `ContextTrace-<version>-windows-x64-cli.zip` — `ct.exe` and `LICENSE`;
- `SHA256SUMS.txt` — SHA-256 hashes for all downloadable files.

## Woodpecker packaging path

The tag workflow is [`.woodpecker/release-windows.yaml`](../.woodpecker/release-windows.yaml).
It uses the existing `windows/amd64` local agent and stages files on that
machine under:

```text
C:\woodpecker-cache\contexttrace\release-assets\v0.1.0
```

Before pushing the tag, ensure the release workflow has landed on `main` and
the Windows agent has `npm`, Rust, Tauri's Windows prerequisites, and the
WebView2/NSIS build dependencies already used by `windows.yaml`. Then run:

```powershell
git switch main
git pull --ff-only origin main
git tag -a v0.1.0 -m "ContextTrace 0.1.0"
git push origin v0.1.0
```

Watch the Woodpecker tag pipeline. When it succeeds, retrieve the four files
from the staging directory and create a draft GitHub release manually, or use
the Woodpecker release publisher once a GitHub token has been stored as a
repository secret. A GitHub token is separate from the Woodpecker API token.

The NSIS installer is deliberately per-user (`%LOCALAPPDATA%`) and does not
need administrator privileges. The default Tauri WebView2 bootstrapper may
download WebView2 if the operating system does not already provide it.

Before publishing a draft, download its assets and verify the checksum in
PowerShell:

```powershell
Get-FileHash .\ContextTrace-<version>-windows-x64-setup.exe -Algorithm SHA256
Get-FileHash .\ContextTrace-<version>-windows-x64-portable.zip -Algorithm SHA256
Get-FileHash .\ContextTrace-<version>-windows-x64-cli.zip -Algorithm SHA256
```

Also install the NSIS artifact on a clean Windows machine, launch the extracted
portable desktop ZIP, run `ct.exe --help` from the CLI ZIP, and complete the
desktop acceptance checks. The portable desktop ZIP still requires the
Microsoft Edge WebView2 Runtime; it removes the ContextTrace installation step,
not that OS runtime dependency.

Use this acceptance sequence on the clean host:

1. verify both downloaded hashes against `SHA256SUMS.txt`;
2. install without administrator elevation and launch from the Start menu;
3. confirm Codex and Claude Code roots are discovered, search for a known
   project, filter each agent, inspect a session and move between measured turns;
4. resize the native window to 1024×680 and 1440×900 and check focus, contrast,
   scrolling and the composition/contributor panels;
5. install the candidate over the previous private candidate, repeat launch and
   one inspection, then run `ct.exe --help` from the extracted CLI ZIP;
6. leave the candidate installed for the agreed soak and record any crash,
   stale-data or format-drift evidence before publishing.

Local acceptance on 2026-08-01 completed in-place upgrade, uninstall/fresh
reinstall, native layout and real-corpus workflow portions of this sequence.
It does not replace the separate clean-host and downloaded-asset checks.

On 2026-09-24 an isolated same-machine clean-host simulation passed checksum
verification, installation of the prior 0.1.0 candidate, in-place upgrade to
0.1.1, portable desktop startup, and `ct.exe --version` from the extracted CLI
archive. This is useful release evidence, but it is not a substitute for a
separate Windows VM or physical host: WebView2, SmartScreen, user-profile
permissions and a downloaded-artifact path still need that independent pass.

## Optional Windows code signing

The desktop installer, portable desktop executable and portable `ct.exe` are
unsigned by default. If
all three repository secrets below are set, the workflow imports the certificate
only on the Windows runner, passes its discovered thumbprint to Tauri, and signs
the desktop executable and `ct.exe` before they are zipped:

- `WINDOWS_CERTIFICATE_BASE64`: base64-encoded PFX certificate;
- `WINDOWS_CERTIFICATE_PASSWORD`: PFX password.
- `WINDOWS_TIMESTAMP_URL`: the RFC 3161 timestamp service supplied by the
  certificate provider.

Partial signing configuration fails the release instead of falling back to an
unsigned artifact. In signing mode, the workflow fails unless both executables
have a `Valid` Authenticode signature from the imported certificate and a
timestamp. No certificate, thumbprint, or signing command is committed. An
unsigned browser download can show Windows SmartScreen warnings; do not publish
a release as signed unless the release artifact has been independently verified
with the chosen signing service.

