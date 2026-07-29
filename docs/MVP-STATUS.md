# ContextTrace MVP status

Assessment date: **2026-07-29**

## Bottom line

ContextTrace has reached a **functional/private CLI MVP and a working desktop
vertical slice**. A developer who can build the repository can already complete
the core job in the CLI, and the desktop app now covers its highest-value loop:

1. discover and filter Codex CLI or Claude Code sessions;
2. see prompt growth and compaction points;
3. select a measured turn;
4. inspect context composition, confidence and largest contributors;
5. see which local roots are read.

It has **not reached a public, downloadable desktop MVP**. The data path,
fixture-backed IPC, malformed states, keyboard semantics and automated
accessibility checks pass, but the UI has not yet passed installed visual
acceptance. CI is green and an unsigned installer can be produced; Windows
signing, clean-machine install/upgrade acceptance and release-candidate soak
remain.

Realistic distance from a Windows-first public desktop MVP: **two focused
acceptance packages, roughly 2–4 engineering days plus a release-candidate
soak**. The desktop foundation and core diagnostic view exist, so this is no
longer a greenfield UI estimate. The range covers installed 1024/1440px visual
acceptance and clean-machine install/upgrade/release validation. Code-signing
certificate lead time is external to that engineering estimate; no signing
secrets are currently configured.

## Evidence checked for this assessment

| Check | Result on 2026-07-29 |
|---|---|
| Local repository | One local branch (`main`), no changes, stashes, extra worktrees, unmerged commits or unreachable commits before this documentation update |
| Automated tests | 293 Rust tests plus 7 frontend tests |
| Static verification | `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass |
| Build | `cargo build --workspace --release` passes on Rust 1.97.1, Windows/MSVC |
| Binary smoke | `ct 0.1.0` starts and exposes all twelve documented commands |
| Desktop slice | Tauri v2 command bridge compiles; React type-check, 7 tests and production bundle pass; 4 Rust tests cover fixture-backed IPC, caching and errors |
| Desktop performance | Largest local Codex session: 1.43 s cold and 0.2 ms cached; two high-turn Claude sessions: 0.79–1.07 s cold and 0.4–0.5 ms cached |
| Format-drift sweep | 792 sessions, 134,764 events, every type recognised, 2.71 seconds |
| Privacy architecture | No application upload/telemetry code; core-only Tauri capability and local-IPC CSP; read-only adapters |
| Desktop acceptance | Real-corpus budgets, keyboard semantics, loading/empty/error/malformed states and reduced-motion behavior pass automated checks; installed 1024/1440px visual acceptance remains |
| Distribution | Windows CI is green; local NSIS and staged CLI ZIP/checksums pass; draft release workflow exists; signature and clean-machine acceptance remain |
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
| Core user workflow | Pass | The twelve-command CLI covers discovery, inspection, reconstruction, diagnosis, lifecycle, comparison and export. |
| Desktop core workflow | Partial | Session browser, growth, turn selection, composition and contributors work; automated UX/performance gates pass; installed visual acceptance remains. |
| Two-agent support | Pass | Codex CLI and Claude Code adapters work against fixtures and the current local corpus. |
| Honest measurements | Pass with limitation | Inline-image accounting is threshold-independent. A 40-session/3,353-turn exact audit could not reproduce the historical over-count and found no generic removal marker; the tool reports such cases as unknown rather than inventing semantics. |
| Local-first safety | Pass | Read-only roots, no application telemetry/upload code, core-only desktop capability, local-IPC CSP, secret scan and redacted export are implemented. |
| Repeatable verification | Pass | The first main-branch CI run passes Rust 1.88/stable, all tests, frontend production build, desktop compilation, release workspace build and two-agent fixture smoke. |
| Installation and legal basics | Partial | MIT license, per-user NSIS, CLI ZIP, checksums and draft release automation exist; add a trusted Windows signature and pass clean-machine install/upgrade checks. |
| Release documentation | Partial | README covers candidate installation, upgrade and known limitations; operator procedure is documented; final release notes still need clean-machine validation. |

## Recommended order

1. **CT-044 — complete installed desktop acceptance.** Automated performance,
   IPC, malformed-state, keyboard and accessibility gates pass. Exercise the
   built installer at 1024/1440px and accept focus, contrast, scrolling and the
   discovery-to-diagnosis flow on a clean Windows machine.
2. **CT-043 — sign and ship 0.1.0.** Provision the certificate/timestamp
   service, verify the signature and checksums, exercise install/upgrade plus
   CLI ZIP, soak the candidate, then publish the draft release.

Cost projection (CT-026), exact compaction diffs (CT-027), family trees and
persistent SQLite come after this sequence unless desktop acceptance produces
evidence that one is necessary for the MVP experience.

## Main risks

- **Agent formats move.** `ct doctor --dir` and CI provide detection, but a
  release cadence is still needed to keep installed binaries current.
- **Reconstruction has an evidence ceiling.** Some historical agent logs
  accounted for more content than the observed prompt held. The current exact
  corpus cannot reproduce the case and logs expose no generic removal lineage;
  the tool refuses to turn that absence into a fake residual.
- **A local corpus can create false confidence.** It is broad and valuable, but
  it represents one developer's workloads and installed agent versions.
- **Desktop acceptance can still expose visual defects.** The largest real
  sessions and automated keyboard/accessibility states pass, but installed
  focus, contrast, scrolling and 1024/1440px layouts have not been accepted.
- **Distribution may expose platform assumptions.** The NSIS artifact builds
  locally on Windows/MSVC, but a signed clean-machine install and upgrade have
  not yet been demonstrated.
