# Maintainer handoff

Updated: **2026-10-08**. The maintainer confirmed all seven original parser-audit follow-ups plus release work as eight tasks.

## v0.1.15 cache/context metrics and instruction notifications

Claude prompt snapshots no longer imply instruction drift, and historical
instruction findings do not replay on appends. Codex cache reads are preserved
for pricing. The desktop shows cache token counts and coverage plus used/total
context for the selected model. Unsupported or incomplete billing usage stays
unpriced. See [release notes](docs/releases/v0.1.15.md).

Local development checks passed: 508 Rust tests (one ignored), formatting and
strict Clippy, 180 frontend tests, the frontend build, all 17 fixture entries,
validator regressions and PowerShell 5.1 release-note checks. Woodpecker push
validation and tag publication are pending. Separate clean-host installer/updater
acceptance and native desktop visual QA remain unverified.

## v0.1.14 automatic model pricing

Spend & Forecast now loads LiteLLM's public prices automatically, using recorded
catalog revisions for historical usage and current rates for forecasts. Prices
and revision indices are cached locally. Missing models or required rates stay
explicitly unpriced; no conversation data or model names are uploaded. See
[release notes](docs/releases/v0.1.14.md) for pricing scope and historical-date limits.

Release commit `9018caf7637e86fdec944c1236bbbe930bca2955` passed all
configured frontend, Rust/MSRV and Windows desktop/CLI checks in
[Woodpecker push pipeline 107](https://ci.aneskurtovic.com/repos/5/pipeline/107).
CI recorded 502 Rust tests passed (one ignored) and 178 frontend tests passed.
[Tag pipeline 108](https://ci.aneskurtovic.com/repos/5/pipeline/108) packaged and
published [v0.1.14](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.14),
verified all six uploaded asset digests and confirmed the latest stable pointer.
Independent public checks on 2026-10-08 confirmed stable publication, all six
asset digests against the downloaded checksum manifest, and the public updater
feed's version, pinned installer URL and matching signature metadata.
Separate clean-host installer/updater acceptance and native desktop visual QA
remain unverified. Metadata checks do not verify an in-app update or Windows
Authenticode signature; see [release procedure](docs/RELEASING.md).

## v0.1.13 project organization and native resume

Temporary workspaces are grouped in the project dropdown, with reversible
visibility preferences. Session rows offer native Codex/Claude resume with
folder validation, command preview/copy and subagent parent routing. See
[projects and resume](docs/projects-and-resume.md) for requirements.

Local development checks passed: 495 Rust tests (one ignored), formatting and
strict Clippy, 176 frontend tests with one worker and a 15-second timeout,
the frontend production build, 17 fixture entries, validator regressions and
the PowerShell 5.1 release-note check. A Windows stub CLI exercised the
production resume command builder. Real-agent session restoration, native
desktop visual QA, and clean-host installer/updater acceptance remain unverified.

Release commit `b15cb80dbed1b697579b4ff840b0f3e39ace365a` passed all
configured frontend, Rust/MSRV and Windows desktop/CLI checks in
[Woodpecker push pipeline 104](https://ci.aneskurtovic.com/repos/5/pipeline/104).
CI recorded 495 Rust tests passed (one ignored) and 176 frontend tests passed.
[Tag pipeline 105](https://ci.aneskurtovic.com/repos/5/pipeline/105) packaged and
published [v0.1.13](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.13),
verified all six uploaded asset digests and confirmed the latest stable pointer.
GitHub metadata independently confirmed the six assets and stable publication
on 2026-10-08. Signature metadata checks and clean-host updater acceptance are
distinct; see [release procedure](docs/RELEASING.md).

## v0.1.12 patch release

Local file targets in Context composition now open with their full recorded
paths, with Explorer/copy actions and missing-file states. Notification findings
navigate to Conversation from every tab; new Windows toasts restore or launch
the installed app through the installer-registered protocol.
Commit `303b502` passed all configured checks in
[push pipeline 100](https://ci.aneskurtovic.com/repos/5/pipeline/100).
[Tag pipeline 101](https://ci.aneskurtovic.com/repos/5/pipeline/101) packaged,
published and verified all six assets and the latest stable pointer for
[v0.1.12](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.12).
The v0.1.11 candidate failed before publication; v0.1.12 also fixes
PowerShell 5.1 release-note serialization with a Windows CI regression check.
Validation includes 488 passing Rust tests (one ignored) and 167 passing
frontend tests.
Actual Windows toast clicks, file associations and separate clean-host
installer/updater acceptance remain unverified. See
[release notes](docs/releases/v0.1.12.md) for upgrade behavior and limits.

## v0.1.10 patch release

Claude `away_summary` and `bridge_status` now parse as presentation-only
session events, with a reviewed redacted capture and replay regression.
Commit `78d345a` passed all configured checks in
[push pipeline 93](https://ci.aneskurtovic.com/repos/5/pipeline/93).
[Tag pipeline 94](https://ci.aneskurtovic.com/repos/5/pipeline/94) packaged,
published and verified all six assets for
[v0.1.10](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.10),
confirmed as latest stable on 2026-10-08. The original eight-task evidence below
describes the v0.1.9 milestone; its remaining capture and clean-host gaps persist.

## Original v0.1.9 milestone (historical snapshot)

| Task | Current evidence and remaining gate |
|---|---|
| 1. Exact-commit Woodpecker validation | Release commit `193150f` passed frontend, Rust (including MSRV), Windows desktop/CLI and smoke checks in [pipeline 90](https://ci.aneskurtovic.com/repos/5/pipeline/90). |
| 2. Claude 2.1.293 persisted capture | Fresh reviewed/redacted capture added, with version, usage, DAG and source-position assertions. |
| 3. Codex 0.161.0 capture evidence | Fresh ordinary persisted capture added. Rare newly covered shapes are absent from the available corpus and fresh capture; real feature-matched evidence cannot be supplied from these inputs. Synthetic contracts stay labelled. |
| 4. Image generation and context compaction | Pinned upstream investigation, synthetic contracts and replay regressions implemented. Image bytes stay outside text estimates. An opaque context-compaction item does not invent an eviction boundary. |
| 5. Nested drift detection | Unknown Codex notification and Claude system subtypes warn. Known telemetry stays metadata. Newly exposed Claude away_summary and bridge_status records remain warnings pending semantic evidence. |
| 6. Media/input bounds | Media reference length excluded from text proxies, mixed media refuses exact recount, JSONL raw buffering is bounded, and oversized raw-inspector ranges are rejected before allocation. Truncated records remain warnings. |
| 7. Separate stdout adapters | Explicit CLI import and opt-in directory contracts added for Codex exec, app-server and Claude stdout. Reviewed stdout captures and a pinned synthetic app-server fixture are catalogued separately. Presentation timelines cannot reconstruct omitted request history. |
| 8. Release and clean-host acceptance | [v0.1.9](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.9) published as latest stable. [Pipeline 91](https://ci.aneskurtovic.com/repos/5/pipeline/91) passed the exact-commit gate, Windows packaging, publication and all six uploaded-asset digest checks. The maintainer confirmed no separate clean Windows host/VM is available, so installer, portable, upgrade, uninstall/archive preservation and signed updater acceptance cannot be performed in this session. |

See [the original audit](docs/FORMAT-AUDIT-2026-10-07.md),
[the follow-up report](docs/FORMAT-FOLLOWUP-2026-10-08.md), and
[stream imports](docs/STREAM-IMPORTS.md) for source pins, capture limits,
bounded-input behavior and the explicit import contract. Private corpus files
and raw captures stay local.

## Validation and release

Follow [CLAUDE](CLAUDE.md), [CI](docs/CI.md) and [RELEASING](docs/RELEASING.md).
Never substitute workstation packaging for the Woodpecker Windows tag pipeline.
Local verification is development evidence; clean-host acceptance is separate.

The v0.1.10 local Claude sweep read 43 sessions / 21,574 events, with no
unreadable files and one malformed line. The away_summary/bridge_status
warnings are resolved. Doctor correctly exits 1 for malformed input; do not
suppress that warning to manufacture a green sweep.

The latest stable release is v0.1.13 at `b15cb80dbed1b697579b4ff840b0f3e39ace365a`.
GitHub latest-release metadata was checked on 2026-10-08: all six expected
assets are uploaded, and the release is neither draft nor prerelease.
The remaining work requires external evidence: feature-matched real Codex
captures, a Claude 2.1.294 capture, real-agent resume verification and a
separate clean Windows host. These gaps are not closed by green CI.
Do not describe all eight tasks as fully accepted.
