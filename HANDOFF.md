# Maintainer handoff

Updated: **2026-10-09**. Prepared v0.1.16 from the checked FEEDBACK remediation work. This file and [the backlog](docs/BACKLOG.md) are the continuation entry points; no conversation history is required.

## Release state

v0.1.16 version files and [release notes](docs/releases/v0.1.16.md) are prepared. Exact-commit Woodpecker push validation, tag packaging, publication and public asset verification are pending. Local validation passed 547 Rust tests (one ignored), 187 frontend tests, six final live/context race regressions, formatting, strict Clippy, the frontend build, 17 fixtures, validator negatives and PowerShell 5.1 release-note serialization. Release-candidate reruns passed after removing only the verified repo-local disposable incremental cache: 547 Rust tests (one ignored), 187 frontend tests after npm ci, the production build, formatting and strict Clippy. All 17 fixtures, validator negatives and PowerShell 5.1 release-note serialization passed. Version files agree on 0.1.16; documentation links and the 12 implemented plus 22 open audit partition were checked.

The previous stable release is [v0.1.15](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.15), at 8a72cc76a8f1aec04911dc31d48cc63cb3e34d8c. Live checks confirmed successful [push pipeline 110](https://ci.aneskurtovic.com/repos/5/pipeline/110), [tag pipeline 111](https://ci.aneskurtovic.com/repos/5/pipeline/111) and stable publication.

## Completed implementation

| Commit | Findings | Result |
|---|---|---|
| 2455a2b | CT-101, CT-131 | Separate request input from its own response; refuse cyclic ancestry. |
| 6b49265 | CT-104, CT-105 | Immutable acknowledged archive revisions, writer coordination and incomplete-tail recovery. |
| c1180af | CT-102 | Require complete applicable billing buckets; keep absent usage unpriced. |
| 8df3b23 | CT-118 | Preserve decoded JSON credential context while redacting encoded spans. |
| 11a40bf | CT-110 | Direct Open only for supported data extensions; canonical target revalidation. |
| b46acf6 | CT-111 | Injected 4 MiB local regular-file instruction reader and typed refusals across desktop/CLI/MCP. |
| 2d8b4b5 | CT-103 | Live descriptor observation independent of alerts/settings; compound identity and failed-emission retry. |
| 85f84aa | CT-133 | Per-block Claude operation/error accounting without splitting usage or graph nodes. |
| 4fb1752 | CT-121 | Compound notification keys and retained-record migration; format drift stays agent-wide. |
| 6c44916 | CT-106 | Metadata/conversation/save controls survive optional context failure; stale/live guards. |

Twelve of the original 34 findings have local remediation checkpoints. [FEEDBACK.md](FEEDBACK.md#remediation-progress) preserves the original audit and implementation evidence; it does not declare the audit closed.

## Next session

1. Read [BACKLOG](docs/BACKLOG.md), [CLAUDE](CLAUDE.md) and the relevant FEEDBACK finding. Audit current branch, remote divergence and uncommitted files before editing; preserve unrelated or recoverable work.
2. Address export scratch/commit writer coordination before CT-109 bounded worker offloading. Archive writer coordination is complete; synchronous native commands and export overlap remain open.
3. Fix CT-119 request/model-window fidelity and CT-120 historical secret/residual alert replay.
4. Improve CT-123 Claude calibration with immutable revision indexes and semantic-equivalence/release-mode performance checks. Then work through the remaining 22 findings in small checked commits.

## Limits that survive this release

- Request reconstruction remains limited by recorded evidence: exact Codex request identity and aggregated Claude iterations are not inferred. Feature-matched captures and producer-version gaps remain in the compatibility catalog.
- Unreferenced archive copies are retained; general orphan catalog reconciliation and power-loss rehearsal remain open.
- The instruction reader refuses direct network/device paths, Windows remote drives, links/reparse points, non-regular files and excessive bodies. Concurrent ancestor replacement is not race-proof; Unix mount locality is not determined.
- File-extension policy does not establish Windows reader behavior. Legacy notification tombstones without retained records cannot be assigned an agent reliably; CT-120 replay work remains open.
- Separate clean-host installer, upgrade, uninstall/archive preservation, portable startup, signed in-app updater and Authenticode acceptance remain unverified. Installed toast activation, file associations, alerts-off Follow live, partial-context browsing and real Codex/Claude resume also need native acceptance.

## Verification and release policy

Always use Woodpecker. Exact main-commit push checks precede version-tag Windows packaging, publication, six-asset digest checks and stable metadata verification. Never run/dispatch GitHub Actions or substitute workstation packaging for release evidence. Follow [RELEASING](docs/RELEASING.md) and keep clean-host acceptance distinct from CI/publication.

Historical parser-audit/capture evidence remains in [the 2026-10-07 audit](docs/FORMAT-AUDIT-2026-10-07.md), [the follow-up](docs/FORMAT-FOLLOWUP-2026-10-08.md), [compatibility](docs/FORMAT-COMPATIBILITY.md) and [stream imports](docs/STREAM-IMPORTS.md). Private corpus files, tokens, signing material and runner-specific secrets stay out of documentation.
