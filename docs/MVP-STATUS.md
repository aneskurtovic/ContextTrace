# ContextTrace MVP status

Assessment date: **2026-09-24**

This assessment is for the published `main` release line at `0.1.1`. Historical corpus,
performance and installation measurements below retain their original dates.

## Bottom line

ContextTrace has reached a **functional/private CLI MVP and an installed desktop
MVP candidate**. A developer can complete the core job in either surface, and
the desktop app covers its highest-value loop:

1. discover and filter Codex CLI or Claude Code sessions;
2. see prompt growth and compaction points;
3. select a measured turn;
4. inspect context composition, confidence and largest contributors;
5. trace a contributor's observed lifetime and departure across the session;
6. opt into exact-duplicate, low-entropy and potential-secret diagnosis;
7. see which local roots are read.

It has **not reached a public, downloadable desktop MVP**. Local source gates
pass, and the current remote Rust lane is green; the
unsigned installer upgrades and launches locally, and installed 1024×680 and
1440×900 acceptance now passes. A clean Windows host, downloaded release-asset
and portable-CLI validation, and release-candidate soak remain. Windows signing
is deliberately deferred until the production-release discussion.

The current 0.1.1 candidate also passed an isolated same-machine clean-host
simulation: checksum verification, prior 0.1.0 installation, in-place upgrade,
portable desktop startup and CLI archive execution. A separate Windows VM or
physical host and a downloaded-artifact path remain release gates.

Realistic distance from a Windows-first public desktop MVP: **one clean-machine
release-candidate session (roughly half to one hands-on day), followed by a
short soak**. There is no known core product-code gap. Code-signing certificate
lead time is external to that estimate; no signing secrets are configured, and
certificate procurement is deferred until the production-release decision.

## Evidence checked for this assessment

| Check | Result through 2026-09-24 |
|---|---|
| Local repository | `main` matches `origin/main` at `08dddd3`; no stashes or extra worktrees. Git reports one metadata-only Cargo manifest change whose normalized content matches `HEAD`; the managed workspace cannot refresh the read-only `.git` index. Unreachable historical commits remain and were audited. |
| Automated tests | Current local run: 447 Rust tests passed, 1 ignored, and 139 frontend tests passed. |
| Static verification | Current local `cargo fmt --all -- --check` and non-desktop `cargo clippy --workspace --exclude ct-ui --all-targets -- -D warnings` pass. Locked workspace checking passes on the installed stable toolchain. |
| Build | Current frontend type-check and production Vite build pass; locked `0.1.1` workspace release build and Tauri `--no-bundle` compile pass. Clean-machine installer build remains a release-candidate gate. |
| Binary smoke | Current `0.1.1` release CLI reports its version and the CLI, secret-redaction and archive-integrity Windows smokes pass. |
| Desktop slice | Current full-workspace Rust tests pass, including 43 `ct-ui` tests; frontend production build passes. Native install/upgrade and viewport evidence below is historical until a new candidate is exercised. |
| Desktop performance | Largest local Codex session (94.6 MiB), on the content-analysis path the doctor view and `ct context` run: 1.26–1.28 s warm over three runs, 1.71 s on a single cold observation, 0.1 ms cached. Two high-turn Claude sessions, plain load: 0.79–1.07 s cold and 0.4–0.5 ms cached. Warm and cold are separate measurements and neither substitutes for the other |
| Format-drift sweep | Current release CLI doctor sweep: 131 Codex sessions / 44,340 events and 10 Claude Code sessions / 4,256 events; every observed type recognised, no unreadable files. |
| Privacy architecture | No application upload/telemetry code; core-only Tauri capability and local-IPC CSP. Agent directories are read-only. ContextTrace-owned writes go under `%LOCALAPPDATA%\ContextTrace-archive`: explicit archive/export copies, durable notification state, the remembered corpus sweep and exports. The directory is a sibling of the install directory, so uninstall cannot take archived evidence with it; `ct roots` names the one write root, and credential shapes are replaced on ingest unless `--raw` is asked for and recorded. |
| Desktop acceptance | Real-corpus budgets, race handling, measurement-limit copy, keyboard semantics, loading/empty/error/malformed states and reduced motion pass; installed search and Codex/Claude filtering plus native 1024×680/1440×900 layouts pass against 816 sessions. Context Doctor's new responsive panel passes the same two viewport widths. |
| Desktop differentiation | On-demand Context Doctor exposes exact repeats, low-entropy ranking and value-free potential-secret locations. Contributor rows open an observed lifecycle with compaction/branch/unknown departure semantics. The desktop also archives a session and verifies a copy, exports NDJSON, compares turns across two sessions, estimates local cost with an explicit forecast, compares recorded instruction files and shows temporal context ghosts. |
| Distribution | Woodpecker pipeline 44 is green across frontend, Rust format/Clippy/tests/MSRV and Windows smokes. Source is 0.1.1; the only local tag remains `v0.1.0`. The isolated candidate check passed; independent clean-host and downloaded-asset acceptance remain. |
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
| Core user workflow | Pass | The eighteen-command CLI covers discovery, inspection, reconstruction, exact Codex compaction diffs, diagnosis, lifecycle, comparison, evidence tools and export. |
| Desktop core workflow | Pass | Paged search, growth, race-safe turn selection, honest measurement limits, composition, contributors, lifecycle drill-down and opt-in Context Doctor pass automated acceptance; the previous installed native slice passed installer lifecycle acceptance. |
| Two-agent support | Pass with scope | Persisted Codex CLI and Claude Code session JSONL work against fixtures and the current local corpus. Codex 0.156.1 and Claude Code 2.1.282 real persisted shapes are catalogued, with 2.1.268 retained as historical coverage. “All versions” is not certified; the compatibility manifest defines the evidence required per producer version and keeps stdout/app-server streams out of this claim. |
| Honest measurements | Pass with limitation | Inline-image accounting is threshold-independent. A 40-session/3,353-turn exact audit could not reproduce the historical over-count and found no generic removal marker; the tool reports such cases as unknown rather than inventing semantics. |
| Local-first safety | Pass | Read-only roots, no application telemetry/upload code, core-only desktop capability, local-IPC CSP, secret scan and redacted export are implemented. |
| Repeatable verification | Pass with release caveat | Local checks, current-corpus doctor sweeps and the fixture manifest pass; the current remote Rust lane is green. A separate Windows host and downloaded-artifact acceptance remain before a public binary release. |
| Installation and legal basics | Pass with release caveat | MIT license, per-user NSIS, CLI ZIP, checksums and draft release automation exist; the isolated 0.1.0-to-0.1.1 install/upgrade and portable/CLI smoke passed. Separate-host acceptance and signing remain deferred until the production-release decision. |
| Release documentation | Pass | README is a front door (~300 lines: what it is, quickstart, install, commands, roadmap) with the engineering narrative moved to `docs/guide.md`, `docs/formats.md`, `docs/methodology.md` and `docs/architecture.md`; candidate installation, upgrade and known limitations are covered; operator procedure is documented. Final release notes still need clean-machine validation as part of CT-043's release-candidate soak. |

## Recommended order

1. **Finish the release evidence.** Run the downloaded candidate on a separate
   Windows VM/host and record the independent acceptance results.
2. **CT-043 — validate and ship 0.1.1.** Exercise the downloaded installer,
   upgrade, portable CLI and checksums on a clean Windows host, then soak the
   unsigned private candidate. Before a
   production release, decide on and provision the certificate/timestamp
   service; the workflow will sign and verify both executables.

The next product queue after release validation is intentionally empty until
the clean-machine evidence is complete. Cost projection, local pricing and
forecasting, family trees, fidelity trends, instruction signatures and
instruction-file comparisons, category composition drill-down and temporal
ghost views are implemented. Exact Codex compaction diffs (CT-027) are implemented.

## Main risks

- **Agent formats move.** `ct doctor --dir` and CI provide detection, but a
  release cadence is still needed to keep installed binaries current. The
  compatibility catalog and validator make fixture drift visible, but they do
  not capture upstream sessions automatically.
- **“All versions” is too broad to promise.** The release claim must name the
  persisted input surface and producer versions actually verified; future
  versions remain best-effort until a redacted fixture and semantic tests exist.
- **Reconstruction has an evidence ceiling.** Some historical agent logs
  accounted for more content than the observed prompt held. The current exact
  corpus cannot reproduce the case and logs expose no generic removal lineage;
  the tool refuses to turn that absence into a fake residual.
- **A local corpus can create false confidence.** It is broad and valuable, but
  it represents one developer's workloads and installed agent versions.
- **Distribution may expose platform assumptions.** The NSIS artifact upgrades
  and runs locally on Windows/MSVC, but a separate clean Windows installation
  and downloaded-asset pass have not yet been demonstrated.

