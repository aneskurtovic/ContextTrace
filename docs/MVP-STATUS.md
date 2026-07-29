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

It has **not reached a public, downloadable desktop MVP**. The data path has
passed real-corpus performance and fixture-backed IPC acceptance, but the UI has
not yet passed visual, accessibility, malformed-state or installed-app
acceptance. One evidence-bound accounting investigation remains, the new CI
workflow needs its first green run, and no installer or license is shipped.

Realistic distance from a Windows-first public desktop MVP: **four focused work
packages, roughly 5–8 engineering days plus a release-candidate soak**. The
desktop foundation and core diagnostic view exist, so this is no longer a
greenfield UI estimate. The range covers interaction/accessibility acceptance,
the remaining evidence investigation, CI closure and installer work. Code
signing certificate lead time, if one is not already available, is external to
that engineering estimate.

## Evidence checked for this assessment

| Check | Result on 2026-07-29 |
|---|---|
| Local repository | One local branch (`main`), no changes, stashes, extra worktrees, unmerged commits or unreachable commits before this documentation update |
| Automated tests | 293 Rust tests plus 2 frontend tests |
| Static verification | `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass |
| Build | `cargo build --workspace --release` passes on Rust 1.97.1, Windows/MSVC |
| Binary smoke | `ct 0.1.0` starts and exposes all twelve documented commands |
| Desktop slice | Tauri v2 command bridge compiles; React type-check, tests and production bundle pass; 4 Rust tests cover fixture-backed IPC, caching and errors |
| Desktop performance | Largest local Codex session: 1.43 s cold and 0.2 ms cached; two high-turn Claude sessions: 0.79–1.07 s cold and 0.4–0.5 ms cached |
| Format-drift sweep | 792 sessions, 134,764 events, every type recognised, 2.71 seconds |
| Privacy architecture | No application upload/telemetry code; core-only Tauri capability and local-IPC CSP; read-only adapters |
| Desktop acceptance | Real-corpus data-path budgets pass; installed visual, keyboard, accessibility and malformed-state acceptance remain |
| Distribution | Windows CI exists pending its first GitHub run; no installer, release artifacts, checksums or install instructions |
| Legal packaging | Manifests say MIT, but the repository has no `LICENSE` file |
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
| Desktop core workflow | Partial | Session browser, growth, turn selection, composition and contributors work; real-session UX/performance and installed-app acceptance remain. |
| Two-agent support | Pass | Codex CLI and Claude Code adapters work against fixtures and the current local corpus. |
| Honest measurements | Conditional | Inline-image accounting is now threshold-independent. CT-031 must either identify the remaining over-count mechanism or finalize it as an explicit, tested limitation. |
| Local-first safety | Pass | Read-only roots, no application telemetry/upload code, core-only desktop capability, local-IPC CSP, secret scan and redacted export are implemented. |
| Repeatable verification | Partial | CI now covers Rust 1.88/stable, frontend checks, desktop compilation, release builds and fixture smoke flows; its first GitHub run must pass before this gate closes. |
| Installation and legal basics | Fail | Add the intended license text, produce a signed Windows desktop installer and CLI archives/checksums, and document installation and upgrade. |
| Release documentation | Partial | README and backlog now describe the real state; 0.1 still needs concise known limitations and release notes tested on a clean machine. |

## Recommended order

1. **CT-042 — land and pass the automated release gate.** CI now describes the
   Rust, frontend, desktop, release and fixture-smoke checks; its first GitHub
   run must prove that description.
2. **CT-044 — complete desktop MVP acceptance.** Performance budgets and IPC
   tests pass; close malformed states and pass keyboard, accessibility plus
   1024/1440px installed visual review.
3. **CT-031 — close the reconstruction over-count investigation.** A truthful,
   tested “unresolvable from available logs” is an acceptable result; invented
   semantics are not.
4. **CT-043 — ship 0.1.0.** Add the license, signed Windows installer, CLI
   archives/checksums, install/upgrade instructions, known limitations and
   clean-machine smoke tests.

Cost projection (CT-026), exact compaction diffs (CT-027), family trees and
persistent SQLite come after this sequence unless desktop acceptance produces
evidence that one is necessary for the MVP experience.

## Main risks

- **Agent formats move.** `ct doctor --dir` is the right detector, but without
  CI and a release cadence a detected change can still leave users on a stale
  binary.
- **Claude Code reconstruction has an evidence ceiling.** Roughly one session
  in seven can account for more content than the observed prompt held. The tool
  refuses to turn that into a fake residual, which contains the harm but does
  not explain the mechanism.
- **A local corpus can create false confidence.** It is broad and valuable, but
  it represents one developer's workloads and installed agent versions.
- **Desktop acceptance can still expose interaction defects.** The largest real
  sessions meet the data-path budget without indexing, but keyboard,
  accessibility, malformed-state and installed visual checks remain.
- **Distribution may expose platform assumptions.** The code and Tauri command
  bridge build locally on Windows/MSVC; an installed desktop artifact and clean
  Windows machine have not yet been demonstrated.
