# Continuous integration

ContextTrace is built by the self-hosted [Woodpecker CI](https://woodpecker-ci.org/)
instance at `ci.aneskurtovic.com`. The shared services are described by the
separate `infra` repository; this repository carries only its own
[`.woodpecker/`](../.woodpecker) pipelines.

Every job used to run on GitHub's `windows-latest`. Most of them did not need to.

---

## 1. What runs where

| Workflow | Agent | Backend | What it proves |
|---|---|---|---|
| [`frontend.yaml`](../.woodpecker/frontend.yaml) | shared Linux box | Docker | `tsc`, `vitest` and the production `vite` bundle |
| [`rust.yaml`](../.woodpecker/rust.yaml) | shared Linux box | Docker | Formatting across the whole workspace, and that the core and CLI crates lint, test and hold their 1.88 MSRV |
| [`windows.yaml`](../.woodpecker/windows.yaml) | owner's Windows machine | `local` | The Tauri desktop app compiles, the **whole** workspace lints and tests including `ct-ui`, and the CLI fixture smokes pass on real Windows |

The three run in parallel. The pipeline is green only when all three are.

### The measurement that made this split possible

`rust-toolchain.toml` used to carry this sentence:

> *Only Windows is verified: every job in `.github/workflows/ci.yml` and
> `release.yml` runs on `windows-latest`, and nothing has built this workspace
> on macOS or Linux.*

That was accurate and it was never tested. It is now, and the tree turned out to
be portable already:

| Phase, on `rust:1.97-bookworm` | Result |
|---|---|
| `cargo fmt --all -- --check` (whole workspace) | pass |
| `cargo clippy --workspace --exclude ct-ui --all-targets -- -D warnings` | pass, 13s |
| `cargo test --workspace --exclude ct-ui --all-targets` | pass |
| `cargo +1.88.0 check --workspace --exclude ct-ui --all-targets` | pass, 9s |
| `npm ci && npm test && npm run build` on `node:22` | pass — 75 tests, 4 files |

Nothing needed changing. That is not luck: the workspace contains **no
`#[cfg(windows)]` at all**. The single mention is a comment in
`ct-adapters/src/archive.rs` recording that the archive root is resolved
*without* one, by checking `CONTEXTTRACE_ARCHIVE`, then `LOCALAPPDATA`, then
`XDG_DATA_HOME` in order. `clap` and `chrono` both carry
`default-features = false` for reasons written down in `Cargo.toml` — dropping
`anstream`/`windows-sys`, and dropping the OS timezone machinery — which between
them removed the last platform-coupled crates from the graph.

### Why `windows.yaml` still exists

**A green Linux build is evidence about Linux.** Three things only the Windows
agent can establish:

- **The desktop app.** `ct-ui` links a real WebView2 application through Tauri.
  On Linux that crate needs webkit2gtk, gtk3 and libsoup, and building it there
  would produce a second desktop app that this project does not ship.
- **`ct-ui`'s Rust.** Excluded from the Linux clippy and test steps for the same
  reason, so its only lint and test coverage is here.
- **The CLI fixture smokes.** These assert on discovery through Windows path
  handling and on the bytes the archive writes. Running them on Linux would
  exercise the `XDG_DATA_HOME` branch of a tool that ships for Windows.

`cargo fmt --all` still covers `ct-ui` on Linux, because formatting does not
compile.

---

## 2. What this migration changed on purpose

Two reductions and one addition, all deliberate. None of them is a silent
consequence of moving hosts.

**The MSRV lane moved to Linux and narrowed.** GitHub ran a full
`stable` + `1.88.0` matrix of the whole workspace on Windows. The 1.88 lane is
now `cargo +1.88.0 check --workspace --exclude ct-ui` on Linux. An MSRV
regression is almost always a language or std-API regression, which any target
catches; running it on Linux costs no time on the single Windows agent. **What
this no longer catches: an MSRV regression reachable only through `ct-ui`.**

**`cargo build --workspace --release` is no longer run as its own gate.** The
Windows workflow still builds `ct-cli` in release — the smokes need the real
binary — and the desktop step builds `ct-ui` through Tauri. What is no longer
covered is a release-profile-only failure in a crate neither of those reaches.
With `lto = "thin"` and `codegen-units = 1`, that build was the most expensive
step in the pipeline and the least likely to fail alone.

**The smoke assertions became scripts, and got stricter.** See §4.

---

## 3. The release pipeline stays on GitHub Actions

[`.github/workflows/release.yml`](../.github/workflows/release.yml) is
unchanged and still runs on `windows-latest`. This is not an oversight.

It fires on a `v*` tag a few times a year, and it needs `contents: write` to
publish a draft release, `actions/upload-artifact`, `signtool.exe` from the
Windows SDK, and three code-signing secrets. Moving it would mean:

- a long-lived GitHub personal access token stored as a Woodpecker secret, in
  place of the scoped, ephemeral, per-run token Actions mints automatically;
- the PFX certificate and its password passing through the **`local` backend,
  which has no container isolation** — same user, same filesystem as everything
  else on that machine.

That is a worse security position for the one pipeline where it matters most, to
save a runner cost that is already zero. `release.yml`'s Windows runner is free
on GitHub for a public repository and is not on the critical path of anyone's
working day. See [RELEASING.md](RELEASING.md).

---

## 4. The smoke scripts

The three CLI smokes live in [`scripts/ci/`](../scripts/ci) as PowerShell files,
not as commands embedded in the pipeline. They can be run by hand:

```powershell
cargo build --release -p ct-cli
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/smoke-cli.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/smoke-secrets.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/smoke-archive.ps1
```

Three reasons they are files:

1. **They are the security assertions.** `smoke-secrets.ps1` proves no
   credential survives a redacted export; `smoke-archive.ps1` proves none
   reaches the bytes on disk, manifest included. Under the `local` backend a
   step body is handed to a generated shell wrapper, and whether that wrapper
   propagates a mid-script `throw` into a failed step is **not something this
   project has measured**. Each script is therefore launched as its own
   `powershell -File` process, which has exactly one exit code. A check of this
   kind reporting a false green is worse than having no check.
2. **PowerShell inside YAML has already broken this repository.** Commit
   `4ff69ee` repaired a workflow whose `.\target\release\ct.exe` had been written
   through a heredoc that read `\t` and `\r` as control characters, in five
   places. The bare CRs took the whole file out of the YAML parser and *no jobs
   ran at all*. `.woodpecker/windows.yaml` contains no backslash.
3. **The owner can run what CI runs**, on the machine CI runs on.

### The isolation bug this port had to fix

The GitHub version arranged an isolated `CODEX_HOME` for the credential fixture
but left `CLAUDE_CONFIG_DIR` pointing at the real home directory. On a fresh
hosted runner that was harmless, because there was no real home directory.

The Windows agent is the owner's own machine, where `%USERPROFILE%\.claude`
holds hundreds of real sessions. Those would be discovered, and `ct sessions`
returns a bounded page — so the fixture can fall off the list. The failure mode
is a wrong session id, not an error. Commit `4ff69ee`'s message records hitting
exactly this when running the step by hand.

`Use-IsolatedAgentHomes` in [`scripts/ci/common.ps1`](../scripts/ci/common.ps1)
therefore always isolates **both** agent homes and the archive root, in every
script, whichever agent that script asserts on.

The same helper file replaced twelve hand-written repetitions of
`if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }` with one `Invoke-Ct`. Each of
those was load-bearing: without the check a crashed `ct` yields `$null`, which
several assertions would read as "nothing found" rather than as a failure.

### One of the five credential checks was vacuous

Measured while porting, by running an **unredacted** export of the credential
fixture and looking for each value:

| Fixture credential | In an unredacted `ct export`? |
|---|---|
| OpenAI API key | yes |
| Anthropic API key | yes |
| GitHub token | yes |
| AWS access key id | yes |
| private key (`MIIEvQIB…`) | **no** |

`ct export` does not emit `function_call_output` payloads, and the fixture's
private key appears only in one — line 13. `ct secrets` detects it there, and
the archive strips it, but asserting that *value* is absent from an export was a
check that could not fail. It would have gone on passing if `--redact-secrets`
stopped working entirely.

This was inherited from the GitHub workflow, not introduced by the move. Rather
than delete the line, both scripts now establish a **positive control** first:

- `smoke-secrets.ps1` exports unredacted, records which credentials that
  actually emitted, and asserts the redacted export drops exactly those —
  failing outright if the set is empty. The count appears in the step log
  (`none of the 4 export-reachable credential(s) surviving`), so the gap is
  visible on every run instead of implied by a green tick.
- `smoke-archive.ps1` asserts all five are present in the committed fixture
  before asserting none reached the archive. All five are reachable there,
  because the archive copies raw JSONL rather than rendering records.

Both controls are recomputed each run, so they track what the code does rather
than what it did when this was written.

### Verified, not assumed

Before this was committed, on Windows:

- all three scripts pass against the committed fixtures;
- a script that `throw`s exits **1** — checked by passing a `-CtExe` that does
  not exist, because "the assertion holds" and "the assertion can fail" are
  different claims;
- `Assert-NoFixtureSecretIn` fires when handed a leaking export.

Two defects were found this way and would not have been found by reading:
`Join-Path` with three segments is PowerShell 6+ and the agent's step shell is
Windows PowerShell 5.1; and 5.1 raises a terminating `NativeCommandError` when a
native command writes to redirected stderr under `$ErrorActionPreference =
'Stop'`, which `ct export`'s redaction summary does by design.

---

## 5. Caches, and what is deliberately not cached

**No Docker volume, no repository trust flag.** The Linux workflows re-fetch
crates and npm packages each run. The dependency graph is small enough that this
costs seconds, so `volumes:` is not used and **Trusted (Volumes) does not need
to be enabled** for this repository. Woodpecker refuses `volumes:` outright on
an untrusted repository rather than running uncached, so this is a real setting,
not a default.

**The Windows agent keeps a build cache outside the workspace.** Woodpecker's
`local` backend discards the workspace after every pipeline, which would mean a
cold build of a Tauri-sized dependency graph on every push. `windows.yaml`
therefore points `CARGO_TARGET_DIR` and `npm_config_cache` at
`C:/woodpecker-cache/contexttrace/`. `CARGO_INCREMENTAL` stays `0`, so what
carries over is fingerprinted build output rather than incremental state a clean
checkout would not reproduce. The agent runs one workflow at a time, so nothing
else writes there. Delete that directory to force a cold build.

**Expect the first Windows run to be slow** and to approach the repository's
60-minute timeout. Subsequent runs reuse the cache.

---

## 6. The Windows agent

`windows.yaml` carries `labels: {platform: windows/amd64, backend: local}`.
Until an agent advertising those labels connects, that workflow **queues as
pending and the pipeline never completes — yellow, not red.** A yellow pipeline
is not a pass.

Registration is documented once, in VoxMux's `docs/CI.md` §3, because it is the
same agent on the same machine. What ContextTrace adds to that machine's
prerequisites:

- Node.js 22.13+ and npm (`package.json` declares the floor)
- the Tauri prerequisites for a Windows desktop build: MSVC build tools and the
  WebView2 runtime
- `rustup` with `clippy` and `rustfmt`, and the `1.88.0` toolchain only if you
  want to reproduce the MSRV lane locally

Both repositories share one agent running `WOODPECKER_MAX_WORKFLOWS=1`, so their
Windows workflows queue behind each other.

### Badge

Woodpecker's badge is keyed by the repository's numeric id. ContextTrace is
id **5**:

```markdown
[![status](https://ci.aneskurtovic.com/api/badges/5/status.svg)](https://ci.aneskurtovic.com/repos/5)
```

---

## 7. What CI does not cover

Unchanged by this migration, and worth restating because a green pipeline is
easy to over-read:

- **No real corpus.** Every smoke runs against committed synthetic fixtures
  under isolated agent homes. The 816-session figures in
  [MVP-STATUS.md](MVP-STATUS.md) come from the owner's machine, by hand.
- **No installed desktop app.** `windows.yaml` compiles the Tauri binary with
  `--no-bundle`. Installer lifecycle, clean-host installation and the acceptance
  sequence in [RELEASING.md](RELEASING.md) are manual and stay manual.
- **No signing.** Verified only in the release pipeline, and only when the three
  signing secrets are set.
- **No performance evidence.** `desktop_perf` is an example target that compiles
  here; it is not run as a benchmark.
