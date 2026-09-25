# Continuous integration

ContextTrace uses Linux jobs for portable Rust/frontend checks and a Windows
job for the native Tauri/MSVC build and Windows-specific CLI smoke tests. The
Windows job is limited to trusted repository events; do not enable untrusted
pull requests on any self-hosted runner that can access the host or secrets.

## Local checks

Portable checks intended for Linux and Windows:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --exclude ct-ui --all-targets --locked -- -D warnings
cargo test --workspace --exclude ct-ui --all-targets --locked
cargo +1.88.0 check --workspace --exclude ct-ui --all-targets --locked
npm ci --prefix crates/ct-ui
npm test --prefix crates/ct-ui
npm run build --prefix crates/ct-ui
```

The Linux Rust lane excludes `ct-ui`, whose native Tauri/WebView2 application
is built on Windows. On Windows, also run the complete workspace gates:

```powershell
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo build --release --locked -p ct-cli
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/check-fixture-manifest.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/test-fixture-manifest.ps1
```

Run the native desktop build on Windows with MSVC and WebView2 prerequisites:

```powershell
Push-Location crates/ct-ui
try {
    npm ci
    .\node_modules\.bin\tauri.cmd build --no-bundle --ci -- --locked
} finally {
    Pop-Location
}
```

The `--no-bundle` build proves that the production desktop executable compiles;
it does not test NSIS installation, updates, uninstallation or a downloaded
release artifact.

## What CI does not prove

- It does not validate compatibility with every Codex CLI or Claude Code
  version. The committed compatibility catalog and fixtures define the tested
  scope; local corpus sweeps are machine-specific evidence.
- Compilation and CLI fixture smokes do not replace installation, upgrade,
  uninstall or portable-app checks on a clean Windows host.
- A passing unsigned build does not establish that updater signatures,
  published feed metadata or downloadable asset hashes are correct.
- Automated checks do not inspect private user session corpora.

Release-specific gates are listed in [the release procedure](RELEASING.md).
