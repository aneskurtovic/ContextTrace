# Windows release procedure

The release workflow packages one Windows x64 release from an existing version
tag. Push `v<version>` or run **Release Windows artifacts** manually with that
existing tag. The tag must exactly match `version` in
`crates/ct-ui/src-tauri/tauri.conf.json`; the workflow rejects mismatches.

It creates a draft GitHub release containing:

- `ContextTrace-<version>-windows-x64-setup.exe` — the NSIS desktop installer;
- `ContextTrace-<version>-windows-x64-cli.zip` — `ct.exe` and `LICENSE`;
- `SHA256SUMS.txt` — SHA-256 hashes for both downloadable files.

The NSIS installer is deliberately per-user (`%LOCALAPPDATA%`) and does not
need administrator privileges. The default Tauri WebView2 bootstrapper may
download WebView2 if the operating system does not already provide it.

Before publishing a draft, download its assets and verify the checksum in
PowerShell:

```powershell
Get-FileHash .\ContextTrace-<version>-windows-x64-setup.exe -Algorithm SHA256
Get-FileHash .\ContextTrace-<version>-windows-x64-cli.zip -Algorithm SHA256
```

Also install the NSIS artifact on a clean Windows machine, run `ct.exe --help`
from the extracted CLI ZIP, and complete the desktop acceptance checks.

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

Local acceptance on 2026-08-01 completed the in-place per-user install, native
layout and real-corpus workflow portions of this sequence. It does not replace
the separate clean-host and downloaded-asset checks.

## Optional Windows code signing

Both the desktop installer and portable `ct.exe` are unsigned by default. If
all three repository secrets below are set, the workflow imports the certificate
only on the Windows runner, passes its discovered thumbprint to Tauri, and signs
`ct.exe` before it is zipped:

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
