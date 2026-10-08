# ContextTrace MVP status

Assessment date: **2026-10-08**

## Current state

The CLI and Windows desktop workflow are implemented for persisted Codex CLI
and Claude Code session JSONL. The desktop app can discover sessions, inspect
turns and context composition, trace contributors, compare turns, and run
optional diagnostics. Compatibility is evidence-based, not a claim that every
version or streaming interface is supported; see the
[compatibility policy](FORMAT-COMPATIBILITY.md).

The latest public stable release is **v0.1.10**, published 2026-10-08 at
07:09:40 UTC (09:09:40 in Sarajevo). Release commit
`78d345ae4a8b3ffe93269e8d9afe60cb2047e613` passed all frontend, Linux Rust
(including MSRV), Windows desktop/CLI and smoke checks in
[Woodpecker push pipeline 93](https://ci.aneskurtovic.com/repos/5/pipeline/93).
[Tag pipeline 94](https://ci.aneskurtovic.com/repos/5/pipeline/94) passed the
exact-commit gate, packaged and published the release, verified all six
uploaded-asset digests, and confirmed the latest stable release pointer.
GitHub release metadata independently confirms that tag and source commit,
all six uploaded assets, and neither draft nor prerelease status.

The release includes the [original parser audit](FORMAT-AUDIT-2026-10-07.md)
and [eight-task follow-up](FORMAT-FOLLOWUP-2026-10-08.md), plus support for
Claude away_summary recaps and bridge_status remote-control notices. These
remain presentation-only session events with no model-context weight.
Explicit saved-stream
imports provide timeline evidence separately from persisted context replay.
See [HANDOFF](../HANDOFF.md) for the remaining evidence gaps.

## Latest local development verification (2026-10-08)

- Rust workspace: 481 tests passed, one ignored; formatting and strict Clippy passed.
- Frontend: 156 tests passed on a single-worker/15-second-timeout local retry
  after two default-timeout failures; production build passed. The configured
  Woodpecker frontend test command passed normally on the release commit.
- All 17 fixture catalog entries and validator regression checks passed.
- The updated Claude corpus sweep read 43 sessions / 21,574 events with no
  unreadable files. Only the existing malformed JSON line remains unrecognised;
  `doctor` correctly exits 1. Both presentation-notice warnings are resolved.
- Fresh redacted Codex 0.161.0 and Claude 2.1.293 persisted/stdout captures
  have dedicated semantic contracts. The app-server fixture and rare newly
  covered Codex shapes retain explicitly synthetic evidence.

Local verification is development evidence. Woodpecker validation, packaging
and publication are separately verified above. No separate clean Windows host
or VM is available, so version-matched installer, portable, upgrade,
uninstall/archive preservation and signed updater acceptance remain unverified.
Green CI and historical acceptance do not close those gates.

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
| Core CLI and desktop workflows | Implemented; release commit passed all Woodpecker push checks in pipeline 93 |
| Persisted Codex/Claude compatibility | 17 fixtures across five surfaces, current-version captures and semantic tests; rare Codex shapes lack feature-matched real captures |
| Explicit stdout/app-server import | Dedicated CLI timeline contracts; complete request-history reconstruction unavailable for these surfaces |
| Public source documentation | Updated for v0.1.10 and explicit stream imports |
| Windows production build | Successful in Woodpecker push pipeline 93 and tag packaging pipeline 94 |
| Updater package and feed | v0.1.10 signature asset and metadata produced; all six uploaded digests and latest stable pointer verified in pipeline 94 |
| Installed/portable, upgrade and uninstall/archive preservation acceptance | Separate clean host unavailable; no version-matched 0.1.10 acceptance |
| Signed in-app upgrade | Separate clean host unavailable; no version-matched 0.1.10 acceptance |
| CLI user acceptance | Not requested; automated Windows validation is separate from user acceptance |
| Public stable release | [ContextTrace v0.1.10](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.10), checked 2026-10-08 |

The NSIS installer is per-user. Uninstall must preserve the separate
`%LOCALAPPDATA%\ContextTrace-archive` data directory, which may contain the
only copy of archived session evidence. See [installation and release
checks](RELEASING.md).
