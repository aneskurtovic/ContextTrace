# Maintainer handoff

Updated: **2026-10-08**. The maintainer confirmed all seven original parser-audit follow-ups plus release work as eight tasks.

| Task | Current evidence and remaining gate |
|---|---|
| 1. Exact-commit Woodpecker validation | Original audit commit e1b1b30 passed frontend, Rust and Windows in pipeline 88. The new candidate requires its own push checks. |
| 2. Claude 2.1.293 persisted capture | Fresh reviewed/redacted capture added, with version, usage, DAG and source-position assertions. |
| 3. Codex 0.161.0 capture evidence | Fresh ordinary persisted capture added. Rare newly covered shapes are absent from the available corpus and fresh capture; real feature-matched evidence cannot be supplied from these inputs. Synthetic contracts stay labelled. |
| 4. Image generation and context compaction | Pinned upstream investigation, synthetic contracts and replay regressions implemented. Image bytes stay outside text estimates. An opaque context-compaction item does not invent an eviction boundary. |
| 5. Nested drift detection | Unknown Codex notification and Claude system subtypes warn. Known telemetry stays metadata. Newly exposed Claude away_summary and bridge_status records remain warnings pending semantic evidence. |
| 6. Media/input bounds | Media reference length excluded from text proxies, mixed media refuses exact recount, JSONL raw buffering is bounded, and oversized raw-inspector ranges are rejected before allocation. Truncated records remain warnings. |
| 7. Separate stdout adapters | Explicit CLI import and opt-in directory contracts added for Codex exec, app-server and Claude stdout. Reviewed stdout captures and a pinned synthetic app-server fixture are catalogued separately. Presentation timelines cannot reconstruct omitted request history. |
| 8. Release and clean-host acceptance | 0.1.9 candidate prepared. Packaging, publishing and asset verification must run in Woodpecker after exact-commit push checks. The maintainer confirmed no separate clean Windows host/VM is available, so installer, portable, upgrade, uninstall/archive preservation and signed updater acceptance cannot be performed in this session. |

See [the original audit](docs/FORMAT-AUDIT-2026-10-07.md),
[the follow-up report](docs/FORMAT-FOLLOWUP-2026-10-08.md), and
[stream imports](docs/STREAM-IMPORTS.md) for source pins, capture limits,
bounded-input behavior and the explicit import contract. Private corpus files
and raw captures stay local.

## Validation and release

Follow [CLAUDE](CLAUDE.md), [CI](docs/CI.md) and [RELEASING](docs/RELEASING.md).
Never substitute workstation packaging for the Woodpecker Windows tag pipeline.
Local verification is development evidence; clean-host acceptance is separate.

The latest local corpus sweep found no unreadable files, one malformed Claude
line, and newly exposed away_summary/bridge_status records. Doctor correctly
exits 1. Do not suppress these records to manufacture a green sweep.

The public release before this candidate is v0.1.8 (Woodpecker push pipeline 86,
tag pipeline 87). Check current CI and publication state before resuming;
version-matched clean-host acceptance remains unavailable.
