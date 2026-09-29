# ContextTrace MVP status

Assessment date: **2026-09-29**

## Current state

The CLI and Windows desktop workflow are implemented for persisted Codex CLI
and Claude Code session JSONL. The desktop app can discover sessions, inspect
turns and context composition, trace contributors, compare turns, and run
optional diagnostics. Compatibility is evidence-based, not a claim that every
version or streaming interface is supported; see the
[compatibility policy](FORMAT-COMPATIBILITY.md).

The updater-enabled **0.1.5 Windows release is published as stable**. The user
previously confirmed that 0.1.3 opens on the development PC and a separate
laptop, and that its routine startup update check stays quiet when no update
is available or the feed is temporarily unreachable. The 0.1.5 installer,
portable desktop app and in-app upgrade still need separate-host acceptance.

## Verification evidence

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace --locked` passed, including the new Codex compaction
  and Codex/Claude fixture regressions.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `npm test` passed: 150 tests for 0.1.5.
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
- Woodpecker pipeline 79 passed the 0.1.5 frontend, Rust and Windows checks
  on `main`. Pipeline 80 passed the same tag checks and published the 0.1.5
  Windows release after verifying the staged and uploaded assets.

These checks validate 0.1.5 source/build behavior and its published assets.
The user previously confirmed that 0.1.3 installs and starts on two PCs, that
session discovery works at startup in installed and portable desktop apps,
and that uninstall works from both the newer-installer flow and Windows Add or
Remove Programs. That earlier acceptance does not establish 0.1.5 install or
upgrade behavior. CLI user acceptance was not requested. The signed in-app
upgrade flow remains to be tested with 0.1.5.

## Release gates

| Gate | State |
|---|---|
| Core CLI and desktop workflows | Implemented; Woodpecker pipeline 80 passed frontend, Rust and Windows checks for 0.1.5 |
| Persisted Codex/Claude compatibility | Tested fixture/version scope only; future versions need fixtures and semantic assertions |
| Public source documentation | Organized; examples are synthetic and relative links have been checked |
| Windows production build | Passed on the Woodpecker Windows agent in pipeline 80 |
| Signed updater package and feed | v0.1.5 stable; six assets uploaded and checksum-verified by Woodpecker pipeline 80 |
| Installed and portable desktop acceptance | Previously passed for 0.1.3 on a separate laptop; 0.1.5 acceptance remains open |
| Signed in-app upgrade | 0.1.3 to 0.1.5 user-confirmed upgrade remains to be tested |
| CLI user acceptance | Not requested; Woodpecker Windows CLI smoke checks passed in pipeline 80 |
| Public stable release | [ContextTrace v0.1.5](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.5) |

The NSIS installer is per-user. Uninstall must preserve the separate
`%LOCALAPPDATA%\ContextTrace-archive` data directory, which may contain the
only copy of archived session evidence. See [installation and release
checks](RELEASING.md).
