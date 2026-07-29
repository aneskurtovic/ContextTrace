# ContextTrace MVP status

Assessment date: **2026-07-29**

## Bottom line

ContextTrace has reached a **functional/private CLI MVP**. A developer who can
build the repository can already complete the core job:

1. discover a Codex CLI or Claude Code session;
2. find the relevant turn;
3. inspect what occupied its context and where each item came from;
4. identify large, duplicate, low-information or secret-bearing content;
5. trace changes over time or export the session for analysis.

It has **not reached a public, downloadable MVP**. The missing work is narrower
than a new product milestone, but it matters: close two known accounting
boundaries, make verification repeatable in CI, and ship an installable 0.1
artifact with an actual license and install instructions.

Realistic distance: **four focused work packages, roughly 4–7 engineering days
plus a release-candidate soak**. That is about one focused week if the
reconstruction investigation ends in a documented limitation. It becomes longer
only if CT-031 reveals a recoverable adapter defect rather than an unobservable
agent behavior.

A desktop UI is not part of this MVP. If “MVP” means a Tauri desktop product
rather than the CLI, it is materially farther away: the shell, index,
navigation, packaging and UI acceptance work have not started.

## Evidence checked for this assessment

| Check | Result on 2026-07-29 |
|---|---|
| Local repository | One local branch (`main`), no changes, stashes, extra worktrees, unmerged commits or unreachable commits before this documentation update |
| Automated tests | 285 passed: 61 domain, 119 adapter, 68 application, 23 CLI, 14 fixture |
| Static verification | `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass |
| Build | `cargo build --workspace --release` passes on Rust 1.97.1, Windows/MSVC |
| Binary smoke | `ct 0.1.0` starts and exposes all twelve documented commands |
| Format-drift sweep | 792 sessions, 134,764 events, every type recognised, 2.71 seconds |
| Privacy architecture | No HTTP client in the production dependency tree; agent roots are read through read-only adapters |
| Distribution | No CI, release workflow, downloadable artifacts, checksums or install instructions |
| Legal packaging | Manifests say MIT, but the repository has no `LICENSE` file |
| crates.io packaging | `cargo package -p ct-cli --no-verify` fails because internal path dependencies have no registry version requirement |

The corpus result is strong evidence for the current machine and current agent
formats. It is not a substitute for CI or for tests on another clean machine.
The corpus itself stays local because it contains prompts, code and secrets.

## Public MVP definition

The public MVP is a **local-first 0.1 CLI release for Codex CLI and Claude Code**.
It is done when a new user can download or build `ct`, run the discovery-to-
diagnosis workflow, understand the confidence and known limitations of every
number, and reproduce the project's verification without access to the
developer's private corpus.

Search, cost projection, SQLite, a desktop UI, more agent adapters and perfect
prompt replay are explicitly outside this release. They may be valuable, but
none is required to prove the core job.

## Release gates

| Gate | State | What remains |
|---|---|---|
| Core user workflow | Pass | The twelve-command CLI covers discovery, inspection, reconstruction, diagnosis, lifecycle, comparison and export. |
| Two-agent support | Pass | Codex CLI and Claude Code adapters work against fixtures and the current local corpus. |
| Honest measurements | Conditional | CT-041 must remove threshold-dependent inline-image accounting. CT-031 must either identify the over-count mechanism or finalize it as an explicit, tested limitation. |
| Local-first safety | Pass | Read-only roots, no telemetry or HTTP client, secret scan and redacted export are implemented. |
| Repeatable verification | Fail | Add CI for formatting, clippy, all tests, a release build and process-level fixture smoke tests. Verify the declared Rust 1.85 minimum or raise it. |
| Installation and legal basics | Fail | Add the intended license text, choose the release channels, build archives/checksums, and document installation and upgrade. |
| Release documentation | Partial | README and backlog now describe the real state; 0.1 still needs concise known limitations and release notes tested on a clean machine. |

## Recommended order

1. **CT-041 — unify inline-image accounting.** It is a bounded correctness
   defect already named by the README and should not cross a release boundary.
2. **CT-031 — close the reconstruction over-count investigation.** A truthful,
   tested “unresolvable from available logs” is an acceptable result; invented
   semantics are not.
3. **CT-042 — automate the release gate.** CI should run format, clippy, all 285
   tests, release builds, and CLI smoke flows over synthetic fixtures. Include
   at least the supported release hosts and the declared minimum Rust version.
4. **CT-043 — ship 0.1.0.** Add the chosen license file, release archives and
   checksums, install/upgrade instructions, known limitations and a clean-machine
   smoke test.

Cost projection (CT-026), exact compaction diffs (CT-027), family trees,
SQLite and Tauri come after this sequence. They add capability; they do not make
the current capability shippable.

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
- **Distribution may expose platform assumptions.** The code builds locally on
  Windows/MSVC and GNU-LLVM; Linux and macOS release artifacts have not yet been
  demonstrated in this repository.
