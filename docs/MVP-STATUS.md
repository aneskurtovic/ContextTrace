# ContextTrace MVP status

Assessment date: **2026-09-25**

## Current state

The CLI and Windows desktop workflow are implemented for persisted Codex CLI
and Claude Code session JSONL. The desktop app can discover sessions, inspect
turns and context composition, trace contributors, compare turns, and run
optional diagnostics. Compatibility is evidence-based, not a claim that every
version or streaming interface is supported; see the
[compatibility policy](FORMAT-COMPATIBILITY.md).

The repository is public, but the current updater-enabled **0.1.2 candidate is
not yet a published release**. Users of 0.1.1 will not see a 0.1.2 update until
its stable assets are published. A separate clean Windows host must validate the
downloaded signed installer/feed and portable artifacts before publication.

## Verification in this working session

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace --locked` passed, including the new Codex compaction
  and Codex/Claude fixture regressions.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `npm test` passed: 148 tests across 5 files.
- `npm run build` passed.
- The fixture manifest and its regression validator passed.
- The Windows production Tauri build (`tauri build --no-bundle --ci --
  --locked`) passed and produced `target/release/context-trace.exe`.

These checks validate source and build behavior on the current development
environment. They are not downloaded-artifact, independent-host, installer
upgrade, or updater-signature acceptance.

## Release gates

| Gate | State |
|---|---|
| Core CLI and desktop workflows | Implemented; workspace and frontend tests pass in this session |
| Persisted Codex/Claude compatibility | Tested fixture/version scope only; future versions need fixtures and semantic assertions |
| Public source documentation | Organized; examples are synthetic and relative links have been checked |
| Windows production build | Passed in this working session |
| Signed updater package and feed | Packaging path prepared; final artifacts not yet staged or published |
| Clean-host installer/upgrade/uninstall | Must be repeated against downloaded 0.1.2 artifacts on a separate Windows host |
| Public stable release | Not published; requires the above acceptance and release assets |

The NSIS installer is per-user. Uninstall must preserve the separate
`%LOCALAPPDATA%\ContextTrace-archive` data directory, which may contain the
only copy of archived session evidence. See [installation and release
checks](RELEASING.md).
