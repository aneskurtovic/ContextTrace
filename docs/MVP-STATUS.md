# ContextTrace MVP status

Assessment date: **2026-09-25**

## Current state

The CLI and Windows desktop workflow are implemented for persisted Codex CLI
and Claude Code session JSONL. The desktop app can discover sessions, inspect
turns and context composition, trace contributors, compare turns, and run
optional diagnostics. Compatibility is evidence-based, not a claim that every
version or streaming interface is supported; see the
[compatibility policy](FORMAT-COMPATIBILITY.md).

The updater-enabled **0.1.2 Windows release is published as stable**. The user
confirmed that the installed app opens on both the development PC and a separate
new laptop. After publication it reports the installed 0.1.2 is current, but
keeps a persistent success banner on screen. Version 0.1.3 changes the updater
to keep routine startup checks quiet when there is no update or the feed is
temporarily unreachable. Its signed assets are in a draft awaiting install and
startup verification.

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
- Woodpecker pipeline 61 uploaded all six 0.1.2 release assets to a GitHub
  release and verified their SHA-256 digests. Creating the draft through the
  GitHub website worked around the release-creation API's HTTP 500 response;
  the release was then published as stable.
- The user installed the downloaded 0.1.2 installer and confirmed the app
  opens correctly. The installed executable's product version is 0.1.2.
- The user separately installed and opened the app on a new laptop.
- The user confirmed the running 0.1.2 app now reports it is up to date, so the
  published stable feed is reachable from the app. Direct shell fetches remain
  blocked by this development environment's network policy.
- Woodpecker pipeline 63 passed all frontend, Rust, Windows desktop and CLI
  smoke checks for the 0.1.3 quiet-updater change.
- Woodpecker pipeline 64 built the signed 0.1.3 Windows package. Pipeline 68
  uploaded all six assets to a draft and verified their checksums.

These checks validate source/build behavior and startup on two PCs. Session
discovery, upgrade/uninstall preservation, portable/CLI use, and the signed
in-app update flow have not yet been confirmed.

## Release gates

| Gate | State |
|---|---|
| Core CLI and desktop workflows | Implemented; workspace and frontend tests pass in this session |
| Persisted Codex/Claude compatibility | Tested fixture/version scope only; future versions need fixtures and semantic assertions |
| Public source documentation | Organized; examples are synthetic and relative links have been checked |
| Windows production build | Passed in this working session |
| Signed updater package and feed | v0.1.2 is stable; v0.1.3 signed assets uploaded and checksum-verified in a draft by Woodpecker pipeline 68; candidate startup confirmation pending |
| Clean-host installer/upgrade/uninstall | v0.1.2 installation/startup passed on a separate new laptop; upgrade and archive-preserving uninstall remain unverified |
| Public stable release | [ContextTrace v0.1.2](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.2); v0.1.3 is in draft acceptance |

The NSIS installer is per-user. Uninstall must preserve the separate
`%LOCALAPPDATA%\ContextTrace-archive` data directory, which may contain the
only copy of archived session evidence. See [installation and release
checks](RELEASING.md).
