# ContextTrace MVP status

Assessment date: **2026-09-25**

## Current state

The CLI and Windows desktop workflow are implemented for persisted Codex CLI
and Claude Code session JSONL. The desktop app can discover sessions, inspect
turns and context composition, trace contributors, compare turns, and run
optional diagnostics. Compatibility is evidence-based, not a claim that every
version or streaming interface is supported; see the
[compatibility policy](FORMAT-COMPATIBILITY.md).

The updater-enabled **0.1.3 Windows release is published as stable**. The user
confirmed the installed app opens on the development PC and a separate new
laptop. On the new laptop, the 0.1.3 app no longer shows the persistent
up-to-date banner. The updater also keeps routine startup checks quiet when no
update is available or the feed is temporarily unreachable.

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
  uploaded all six assets and verified their checksums. The draft was published
  as the stable v0.1.3 release after the user confirmed the quiet updater
  behavior on a separate laptop.

These checks validate source/build behavior, installation and startup on two
PCs, and the quiet updater behavior on the separate laptop. The user also
confirmed that session discovery works at startup in both the installed and
portable desktop apps, the portable desktop app works, and uninstall works
both from the newer-installer flow and Windows Add or Remove Programs. CLI
user acceptance was not requested. The signed in-app upgrade flow will be
tested with the next version.

## Release gates

| Gate | State |
|---|---|
| Core CLI and desktop workflows | Implemented; workspace and frontend tests pass in this session |
| Persisted Codex/Claude compatibility | Tested fixture/version scope only; future versions need fixtures and semantic assertions |
| Public source documentation | Organized; examples are synthetic and relative links have been checked |
| Windows production build | Passed in this working session |
| Signed updater package and feed | v0.1.3 stable; signed assets uploaded and checksum-verified by Woodpecker pipeline 68; quiet startup behavior confirmed on a separate laptop |
| Installed and portable desktop acceptance | Installed startup passed on a separate new laptop; startup session discovery works in installed and portable apps; portable app works; uninstall succeeds from both the installer update flow and Windows Add or Remove Programs |
| Signed in-app upgrade | Pending the next version; user will test the offered upgrade flow then |
| CLI user acceptance | Not requested; Woodpecker CLI smoke checks passed in pipeline 63 |
| Public stable release | [ContextTrace v0.1.3](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.3) |

The NSIS installer is per-user. Uninstall must preserve the separate
`%LOCALAPPDATA%\ContextTrace-archive` data directory, which may contain the
only copy of archived session evidence. See [installation and release
checks](RELEASING.md).
