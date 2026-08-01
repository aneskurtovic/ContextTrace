# ContextTrace MVP status

Assessment date: **2026-08-01**

## Bottom line

ContextTrace has reached a **functional/private CLI MVP and an installed desktop
MVP candidate**. A developer can complete the core job in either surface, and
the desktop app covers its highest-value loop:

1. discover and filter Codex CLI or Claude Code sessions;
2. see prompt growth and compaction points;
3. select a measured turn;
4. inspect context composition, confidence and largest contributors;
5. opt into exact-duplicate, low-entropy and potential-secret diagnosis;
6. see which local roots are read.

It has **not reached a public, downloadable desktop MVP**. CI is green, the
unsigned installer upgrades and launches locally, and installed 1024×680 and
1440×900 acceptance now passes. A clean Windows host, downloaded release-asset
and portable-CLI validation, and release-candidate soak remain. Windows signing
is deliberately deferred until the production-release discussion.

Realistic distance from a Windows-first public desktop MVP: **one clean-machine
release-candidate session (roughly half to one hands-on day), followed by a
short soak**. There is no known core product-code gap. Code-signing certificate
lead time is external to that estimate; no signing secrets are configured, and
certificate procurement is deferred until the production-release decision.

## Evidence checked for this assessment

| Check | Result through 2026-08-01 |
|---|---|
| Local repository | One local branch (`main`), no changes, stashes, extra worktrees, unmerged commits or unreachable commits before this documentation update |
| Automated tests | 298 Rust tests plus 20 frontend tests |
| Static verification | `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass |
| Build | `cargo build --workspace --release` passes on Rust 1.97.1, Windows/MSVC |
| Binary smoke | `ct 0.1.0` starts and exposes all thirteen documented commands |
| Desktop slice | Tauri v2 command bridge compiles; React type-check, 20 tests and production bundle pass; 5 Rust tests cover fixture-backed IPC, caching, errors, Context Doctor and a 501-session search/page contract |
| Desktop performance | Largest local Codex session: 1.43 s cold and 0.2 ms cached; two high-turn Claude sessions: 0.79–1.07 s cold and 0.4–0.5 ms cached |
| Format-drift sweep | 816 sessions (717 Claude Code, 99 Codex), 148,970 events, every type recognised, 5.88 seconds |
| Privacy architecture | No application upload/telemetry code; core-only Tauri capability and local-IPC CSP; read-only adapters |
| Desktop acceptance | Real-corpus budgets, race handling, measurement-limit copy, keyboard semantics, loading/empty/error/malformed states and reduced motion pass; installed search and Codex/Claude filtering plus native 1024×680/1440×900 layouts pass against 816 sessions. Context Doctor's new responsive panel passes the same two viewport widths. |
| Desktop differentiation | On-demand Context Doctor exposes exact repeats, low-entropy ranking and value-free potential-secret locations. On a current 208-turn Codex session, the underlying release paths found 16 duplicate groups, 42 low-entropy blocks and two potential-secret locations in 0.43 s plus 0.55 s. |
| Distribution | Windows CI is green; the 2026-08-01 unsigned NSIS candidate passed in-place upgrade plus local uninstall/fresh reinstall, restored shortcuts/uninstaller and launched successfully; local staged CLI/checksums pass, while clean-machine downloaded-asset acceptance remains |
| Legal packaging | MIT `LICENSE` is present and included in the CLI archive |
| crates.io packaging | `cargo package -p ct-cli --no-verify` fails because internal path dependencies have no registry version requirement |

The corpus result is strong evidence for the current machine and current agent
formats. It is not a substitute for CI or for tests on another clean machine.
The corpus itself stays local because it contains prompts, code and secrets.

## Public MVP definition

The public MVP is a **local-first Windows desktop 0.1 for Codex CLI and Claude
Code, with the CLI retained as a companion interface**. It is done when a new
user can install ContextTrace, run the discovery-to-diagnosis workflow without
a terminal, understand confidence and known limitations, and reproduce the
project's verification without access to the developer's private corpus.

Global full-text search, cost projection, persistent SQLite, more agent adapters
and perfect prompt replay are outside this release unless acceptance produces
evidence that one is required for the core workflow.

## Release gates

| Gate | State | What remains |
|---|---|---|
| Core user workflow | Pass | The thirteen-command CLI covers discovery, inspection, reconstruction, exact Codex compaction diffs, diagnosis, lifecycle, comparison and export. |
| Desktop core workflow | Pass | Paged search, growth, race-safe turn selection, honest measurement limits, composition, contributors and opt-in Context Doctor pass automated acceptance; the previous installed native slice passed local lifecycle acceptance. |
| Two-agent support | Pass | Codex CLI and Claude Code adapters work against fixtures and the current local corpus. |
| Honest measurements | Pass with limitation | Inline-image accounting is threshold-independent. A 40-session/3,353-turn exact audit could not reproduce the historical over-count and found no generic removal marker; the tool reports such cases as unknown rather than inventing semantics. |
| Local-first safety | Pass | Read-only roots, no application telemetry/upload code, core-only desktop capability, local-IPC CSP, secret scan and redacted export are implemented. |
| Repeatable verification | Pass | The first main-branch CI run passes Rust 1.88/stable, all tests, frontend production build, desktop compilation, release workspace build and two-agent fixture smoke. |
| Installation and legal basics | Partial | MIT license, per-user NSIS, CLI ZIP, checksums and draft release automation exist; pass clean-machine install/upgrade checks. Signing is deferred until the production-release decision. |
| Release documentation | Partial | README covers candidate installation, upgrade and known limitations; operator procedure is documented; final release notes still need clean-machine validation. |

## Recommended order

1. **CT-046 — contributor lifecycle drill-down.** Make a large contributor
   open the already-implemented entry/survival/departure timeline.
2. **CT-047 — compaction autopsy.** Connect Codex compaction markers to the
   existing exact replacement-history diff.
3. **CT-048 — turn comparison.** Let users pin a baseline and reuse the
   existing measurement-aware diff engine in the desktop app.
4. **CT-043 — validate and ship 0.1.0.** Exercise the downloaded installer,
   upgrade, portable CLI and checksums on a clean Windows host, then soak the
   unsigned private candidate. Before a
   production release, decide on and provision the certificate/timestamp
   service; the workflow will sign and verify both executables.

These three product items are intentionally ahead of release validation: they
reuse proven local analysis and differentiate the desktop product without new
format inference. Cost projection (CT-026), family trees and persistent SQLite
come after this sequence. Exact Codex compaction diffs (CT-027) are implemented.

## Main risks

- **Agent formats move.** `ct doctor --dir` and CI provide detection, but a
  release cadence is still needed to keep installed binaries current.
- **Reconstruction has an evidence ceiling.** Some historical agent logs
  accounted for more content than the observed prompt held. The current exact
  corpus cannot reproduce the case and logs expose no generic removal lineage;
  the tool refuses to turn that absence into a fake residual.
- **A local corpus can create false confidence.** It is broad and valuable, but
  it represents one developer's workloads and installed agent versions.
- **Distribution may expose platform assumptions.** The NSIS artifact upgrades
  and runs locally on Windows/MSVC, but a separate clean Windows installation
  and downloaded-asset pass have not yet been demonstrated.
