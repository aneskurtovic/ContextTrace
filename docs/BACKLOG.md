# Engineering backlog

Updated: **2026-10-09**. Baseline: [FEEDBACK.md](../FEEDBACK.md), 34 confirmed findings. Twelve have local remediation checkpoints in v0.1.16; **22 remain open**. Original audit evidence stays intact. A green release does not close the full audit.

## Release blocker

Release publication is blocked by the offline Woodpecker Windows runner (agent 4, last contact 2026-10-09 13:09:09 UTC). Main release candidate 8cdaf9bab779f3331de8f959150e041e81980cd2 was pushed. Pipeline 113 passed frontend and Linux Rust/MSRV (486 Rust tests, one ignored), but Windows validation never ran. Pipeline 113 was superseded by the documentation follow-up; validate the latest main pipeline before tagging. No v0.1.16 tag or release has been created; latest stable remains v0.1.15. A documentation follow-up records this state. Before tagging, restore the existing Windows runner, wait for all three push statuses on the latest main SHA to pass, then create/push v0.1.16 and verify its Woodpecker Windows tag pipeline, six assets and stable metadata.

## Next implementation order

1. Coordinate export scratch files and commit destinations before CT-109 worker offloading. Archive writer coordination is implemented; export remains a prerequisite. Verify delayed native I/O, bounded overlap and UI responsiveness separately.
2. Fix notification request-window fidelity (CT-119) and historical deep-analysis replay (CT-120).
3. Improve Claude calibration scaling (CT-123) after preserving branch, compaction and request-input semantics; compare release-mode performance and semantic equivalence.
4. Address remaining partial-availability, request-generation and evidence-presentation findings in focused checked commits.

## Open audit findings

| Finding | Severity | Remaining work and acceptance |
|---|---|---|
| CT-107 | Medium | Use supported per-turn marginal estimates, with absent usage controls. |
| CT-108 | Medium | Bind export completion and errors to the selected compound identity and request generation. |
| CT-109 | High | Coordinate export writers first, then offload heavy work with bounded concurrency; verify native responsiveness. |
| CT-112 | Medium | Keep comparison controls visible on failure and clear obsolete results. |
| CT-113 | Medium | Preserve unknown prompt usage in comparison rankings. |
| CT-114 | Medium | Refresh project options with the session catalog while preserving visibility preferences. |
| CT-115 | Medium | Scope secret locations to their recorded turn in the overview. |
| CT-116 | Low | Dispose late listener subscriptions under unmount and Strict Mode. |
| CT-117 | Low | Correct archive guidance and verify the saved-copy browsing workflow. |
| CT-119 | Medium | Use each request/model window for pressure, growth and corpus analysis. |
| CT-120 | Medium | Cursor-filter historical secret and residual findings on append/restart. |
| CT-122 | Medium | Use current log activity for Codex discovery recency and filters. |
| CT-123 | Medium | Index Claude ancestry once per immutable revision; benchmark equivalent branch/compaction semantics. |
| CT-124 | Low | Reject out-of-range MCP numeric values before conversion. |
| CT-125 | Low | Use agent-qualified identities for family grouping. |
| CT-126 | Low | Retain all compactions assigned to a turn. |
| CT-127 | Low | Honor archive verification JSON output contract. |
| CT-128 | Medium | Expose unpriced coverage in human cost comparisons. |
| CT-129 | Low | Sanitize untrusted terminal controls in CLI metadata output. |
| CT-130 | Low | Distinguish unreadable corpus from confirmed empty discovery. |
| CT-132 | Low | Keep oversized messages visible at the JSONL framing-budget edge. |
| CT-134 | Low | Present unsupported residual analysis as unavailable rather than zero. |

## Implemented checkpoints

CT-101, CT-102, CT-103, CT-104, CT-105, CT-106, CT-110, CT-111, CT-118, CT-121, CT-131, CT-133. See [remediation progress](../FEEDBACK.md#remediation-progress) for regression evidence and limits. v0.1.16 release/CI status is recorded in [MVP status](MVP-STATUS.md) and [handoff](../HANDOFF.md).

## External acceptance and retained limitations

- Separate clean Windows host: install, upgrade, uninstall/archive preservation, portable startup and signed in-app updater acceptance; Authenticode is distinct from updater signature metadata.
- Installed/native UI: toast activation, file associations, alerts-off Follow live, partial-context browsing and real Codex/Claude saved-context resume.
- Compatibility: feature-matched real Codex captures and producer-version gaps remain scoped by the catalog; precise request identity/aggregated iterations are not inferred.
- Recovery/security: general archive orphan reconciliation, power-loss rehearsal, concurrent instruction-path ancestor replacement and Unix mount locality remain unverified.
- Migration: legacy notification tombstones without retained records have no reliable agent provenance. Historical deep-analysis replay remains CT-120 work.

Follow [CLAUDE.md](../CLAUDE.md) for checks and [RELEASING.md](RELEASING.md) for Woodpecker-only releases. Preserve recoverable archives and audit evidence; do not use untrusted fork code on the credential-bearing Windows runner.
