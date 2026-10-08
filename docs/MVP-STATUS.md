# ContextTrace MVP status

Assessment date: **2026-10-08**

## Current state

The CLI and Windows desktop workflow are implemented for persisted Codex CLI
and Claude Code session JSONL. The desktop app can discover sessions, inspect
turns and context composition, trace contributors, compare turns, and run
optional diagnostics. Compatibility is evidence-based, not a claim that every
version or streaming interface is supported; see the
[compatibility policy](FORMAT-COMPATIBILITY.md).

## v0.1.13 project organization and native resume

Temporary workspaces are grouped in the project dropdown, with reversible
visibility preferences. Session rows offer native Codex/Claude resume with
folder validation, command preview/copy and subagent parent routing. See
[projects and resume](projects-and-resume.md) for requirements.

Local development checks passed: 495 Rust tests (one ignored), formatting and
strict Clippy, 176 frontend tests with one worker and a 15-second timeout,
the frontend production build, 17 fixture entries, validator regressions and
the PowerShell 5.1 release-note check. A Windows stub CLI exercised the
production resume command builder. Real-agent session restoration, native
desktop visual QA, and clean-host installer/updater acceptance remain unverified.

At preparation time, v0.1.13 Woodpecker push validation, tag packaging,
publication and uploaded-asset verification are pending. The latest verified
stable release is v0.1.12, described below. Publication evidence will be recorded
after the pipeline completes.

## Previous verified publication

The latest verified public stable release at v0.1.13 preparation is **v0.1.12**, published 2026-10-08 at
08:44:29 UTC. Release commit `303b502383c8f02b2a860b657af3ffb11627ff6f` passed all
frontend, Linux Rust (including MSRV), Windows desktop/CLI and smoke checks in
[Woodpecker push pipeline 100](https://ci.aneskurtovic.com/repos/5/pipeline/100).
[Tag pipeline 101](https://ci.aneskurtovic.com/repos/5/pipeline/101) passed the exact-commit gate, packaged and published
the release, verified all six uploaded-asset digests, and confirmed the latest
stable release pointer. GitHub metadata independently confirms the tag, source
commit, all six assets and neither draft nor prerelease status.

The v0.1.12 patch added [local file actions](local-files.md) in Context composition and
fixes [notification navigation and Windows toast activation](notifications.md).
Actual installed Windows toast and file-association behavior remains a separate
acceptance check.

The release includes the [original parser audit](FORMAT-AUDIT-2026-10-07.md)
and [eight-task follow-up](FORMAT-FOLLOWUP-2026-10-08.md), plus support for
Claude away_summary recaps and bridge_status remote-control notices. These
remain presentation-only session events with no model-context weight.
Explicit saved-stream
imports provide timeline evidence separately from persisted context replay.
See [HANDOFF](../HANDOFF.md) for the remaining evidence gaps.

## Local development verification for v0.1.13 (2026-10-08)

- 495 Rust tests passed, one ignored; formatting and strict workspace Clippy passed.
- 176 frontend tests passed with one worker and a 15-second timeout; production build passed.
- All 17 fixture entries, validator regressions and the PowerShell 5.1 release-note check passed.
- A Windows stub CLI tested the production resume command builder; real-agent
  restoration and native visual QA remain unverified.
- Parser capture evidence remains Codex 0.161.0 and Claude 2.1.293. Upstream
  Claude 2.1.294 was identified; no matching capture was added. See
  [compatibility](FORMAT-COMPATIBILITY.md) for limits.

Local checks are development evidence. v0.1.13 CI and publication are pending.
Separate clean-host installer, portable, upgrade, uninstall/archive preservation,
signed updater, actual toast/file associations and real-agent resume acceptance
remain unverified. Historical acceptance does not close those gates.

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
| Core CLI and desktop workflows | v0.1.13 implemented; exact-commit Woodpecker validation pending |
| Persisted Codex/Claude compatibility | 17 fixtures across five surfaces, current-version captures and semantic tests; rare Codex shapes lack feature-matched real captures |
| Explicit stdout/app-server import | Dedicated CLI timeline contracts; complete request-history reconstruction unavailable for these surfaces |
| Public source documentation | Updated for project organization, native resume and explicit evidence limits |
| Windows production build | v0.1.12 verified in pipelines 100/101; v0.1.13 pending |
| Updater package and feed | v0.1.12 verified in pipeline 101; v0.1.13 packaging/publication pending |
| Installed/portable, upgrade and uninstall/archive preservation acceptance | Separate clean host unavailable; no version-matched 0.1.13 acceptance |
| Windows toast clicks and local file associations | Automated routing/path checks passed; installed Windows behavior remains unverified |
| Signed in-app upgrade | Separate clean host unavailable; no version-matched 0.1.13 acceptance |
| Native session resume | Stub command-builder and dialog tests passed; real-agent restoration and visible installed terminal unverified |
| CLI user acceptance | Not requested; automated Windows validation is separate from user acceptance |
| Public stable release | [ContextTrace v0.1.12](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.12), checked 2026-10-08 |

The NSIS installer is per-user. Uninstall must preserve the separate
`%LOCALAPPDATA%\ContextTrace-archive` data directory, which may contain the
only copy of archived session evidence. See [installation and release
checks](RELEASING.md).
