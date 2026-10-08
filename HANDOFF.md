# Maintainer handoff

Updated: **2026-10-08**. The maintainer confirmed all seven original parser-audit follow-ups plus release work as eight tasks.

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

## Original v0.1.9 milestone

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

The latest local Claude sweep read 43 sessions / 21,574 events, with no
unreadable files and one malformed line. The away_summary/bridge_status
warnings are resolved. Doctor correctly exits 1 for malformed input; do not
suppress that warning to manufacture a green sweep.

The latest stable release is v0.1.10 at `78d345ae4a8b3ffe93269e8d9afe60cb2047e613`.
GitHub latest-release metadata was checked on 2026-10-08 (Sarajevo): all six
expected assets are uploaded, and the release is neither draft nor prerelease.
The remaining work requires external evidence: feature-matched real Codex
captures and a separate clean Windows host. Neither gap is closed by green CI.
Do not describe all eight tasks as fully accepted.
