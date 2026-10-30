# ContextTrace contributor notes

Read [AGENTS.md](AGENTS.md) for repository policy. Use Woodpecker for CI and
release packaging; version-tag packaging, publishing and published-asset
verification belong in its Windows release pipeline. Record separate clean-host
installer and updater acceptance independently from CI results. See the
[release procedure](docs/RELEASING.md) for release security and acceptance.

Run the checks in [CI](docs/CI.md) appropriate to the change and platform.
The [compatibility policy](docs/FORMAT-COMPATIBILITY.md) explains fixture
requirements and evidence limits. Keep real session corpora, credentials and
machine-specific runner paths out of tracked files.

On Windows, use the MSVC toolchain and run the Tauri production build from
`crates/ct-ui`. In Windows PowerShell 5.1, check `$LASTEXITCODE` after native
commands: `$ErrorActionPreference = 'Stop'` does not reliably detect their
failure.
