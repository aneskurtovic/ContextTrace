# ContextTrace MVP status

Assessment date: **2026-10-07**

## Current state

The CLI and Windows desktop workflow are implemented for persisted Codex CLI
and Claude Code session JSONL. The desktop app can discover sessions, inspect
turns and context composition, trace contributors, compare turns, and run
optional diagnostics. Compatibility is evidence-based, not a claim that every
version or streaming interface is supported; see the
[compatibility policy](FORMAT-COMPATIBILITY.md).

The latest public stable release is **v0.1.8**, published 2026-10-01. The
release API was checked on 2026-10-07 and lists all six expected assets. Source
commit `3afd229` has successful Woodpecker push checks in pipeline 86 and tag
release checks in pipeline 87. This check did not download or rehash assets.
Separate-host installer, portable and signed updater acceptance for 0.1.8 has
not been established in this audit. Historical acceptance is recorded below.

The 2026-10-07 parser audit is a source change after that release, not part of
the published 0.1.8 binaries. See [the audit](FORMAT-AUDIT-2026-10-07.md) and
[HANDOFF](../HANDOFF.md) for implementation details and next steps.

## Latest local development verification (2026-10-07)

- Rust workspace: 468 tests passed, one ignored; formatting and workspace Clippy passed.
- Frontend: 156 tests passed with one thread and a 15-second test timeout after
  worker startup/timing failures; the production build passed. Existing dependencies were used.
- All 11 fixture catalog entries and validator regression checks passed.
- The final corpus sweep read 229 sessions / 100,651 events, with no unreadable
  files. One malformed Claude record remained correctly flagged; `doctor` exited 1.
- The synthetic Codex 0.161.0 contract does not certify all features; Claude
  2.1.293 still needs a version-matched persisted capture.

These are local source checks. The push containing these changes needs its own
exact-commit Woodpecker statuses; the older pipelines below do not certify it.
No new packaging, publication or clean-host acceptance was performed.

## Historical release and acceptance evidence

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
| Core CLI and desktop workflows | Implemented; v0.1.8 source passed Woodpecker push pipeline 86; parser-audit changes require their own push checks |
| Persisted Codex/Claude compatibility | 2026-10-07 audit and 11 fixtures; current-version capture and semantic coverage gaps remain |
| Public source documentation | Organized; examples are synthetic and relative links have been checked |
| Windows production build | Successful v0.1.8 Woodpecker Windows checks; tag release pipeline 87 succeeded |
| Signed updater package and feed | v0.1.8 stable; six expected assets listed and Woodpecker release status successful; downloaded bytes not reverified in this audit |
| Installed and portable desktop acceptance | Historical 0.1.3 acceptance; no version-matched 0.1.8 acceptance established here |
| Signed in-app upgrade | Version-matched 0.1.8 clean-host acceptance remains unverified here |
| CLI user acceptance | Not requested; automated Windows validation is separate from user acceptance |
| Public stable release | [ContextTrace v0.1.8](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.8), checked 2026-10-07 |

The NSIS installer is per-user. Uninstall must preserve the separate
`%LOCALAPPDATA%\ContextTrace-archive` data directory, which may contain the
only copy of archived session evidence. See [installation and release
checks](RELEASING.md).
