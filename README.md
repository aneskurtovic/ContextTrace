# ContextTrace

**DevTools for AI coding-agent context.**

ContextTrace is a local-first tool for inspecting what an AI coding agent
actually had in its context window, turn by turn. It answers:

> What was in the model's context at this turn, where did it come from, how
> large was it, and how did it evolve during the session?

It is not a chat-history viewer. The workflow it exists for is:

> *Why did the agent do that?* → inspect the turn → see the context → trace each
> item to its source → find the 38k-token garbage tool result → understand the
> behaviour.

Supported agents: **OpenAI Codex CLI** and **Anthropic Claude Code**.

> **Status: early construction.** The domain core and the Codex adapter are
> implemented and tested. The Claude Code adapter, application layer and CLI are
> in progress. See [Current state](#current-state).

---

## Principles

**Local-first.** Session data contains source code, prompts, terminal output and
potentially secrets. Nothing is uploaded, there is no telemetry, and there is no
cloud dependency. The dependency tree deliberately contains no HTTP client.

**Read-only.** Agent directories are inputs. ContextTrace never writes to them.

**Observed vs reconstructed.** Agent logs are not a perfect record of the API
request. Every figure carries its provenance, and the distinction is enforced by
the type system rather than by documentation — see
[`TokenCount`](crates/ct-domain/src/model/tokens.rs), whose variants presentation
code must match on before it can extract a number.

**Adapter-based.** Agent formats are foreign models behind anti-corruption
layers. Adding Cursor, Gemini CLI or OpenCode means implementing one trait and
adding one line to the composition root.

---

## Architecture

Ports and adapters, with the dependency rule enforced by Cargo rather than by
convention.

```
                  ct-cli  (driving adapter + composition root)
                 /   |   \
                /    |    \
  ct-application     |     ct-adapters   (Codex ACL, Claude Code ACL,
           |         |    /               tokenizers, filesystem)
            \        |   /
              ct-domain   (entities, value objects, aggregates,
                           domain services, and the port traits)
```

Every arrow points inward. `ct-domain` depends on nothing but `serde` and
`chrono`. `ct-adapters` implements ports declared in `ct-domain` and never calls
into `ct-application`. Only `ct-cli` knows which concrete adapters exist, and its
job at the boundary is to construct them and inject them.

| Crate | Responsibility |
|---|---|
| `ct-domain` | Entities, value objects, the `AgentSession` and `ContextSnapshot` aggregates, domain services, port traits. No I/O. |
| `ct-application` | Use cases orchestrating domain services over ports. |
| `ct-adapters` | Driven adapters: per-agent ACLs, token estimators, filesystem raw-event source. |
| `ct-cli` | The `ct` binary: driving adapter and composition root. |

### Two invariants worth knowing

**Confidence never launders upward.** Combining an observed fact with an
estimated one yields an estimate. There is no path by which a guess becomes a
measurement.

**A context snapshot always adds up.** `ContextSnapshot` can only be built with
item tokens plus the unattributed residual equalling the reported total. An
inconsistent breakdown is unrepresentable, not merely discouraged.

---

## What the format investigation established

The design rests on the real on-disk formats, probed against a local corpus of
709 Claude Code sessions (336 MB) and 61 Codex sessions (191 MB).

### Codex CLI

`response_item` lines **are** the literal OpenAI Responses API items. Replaying
them reproduces the request body, so context membership is *observed*.
`session_meta.base_instructions` stores the system prompt verbatim. `compacted`
payloads carry `replacement_history` — the post-compaction item list in full —
so discarded content is derivable by diffing rather than merely inferable.

### Claude Code

`usage` on each assistant message gives the **exact** prompt size:
`input_tokens + cache_creation_input_tokens + cache_read_input_tokens`. Reading
only `input_tokens` is the easiest way to be badly wrong — on a warm cache it
reads `2` for a turn carrying 280,000 tokens.

Events form a **DAG** via `parentUuid`; line order is not conversation order.
One API response spans several lines sharing a `requestId`. `attachment` lines
label injected context with its origin, making instruction provenance observed
data rather than inference.

### The asymmetry that shapes everything

Codex is GPT-family, so `tiktoken` can count exactly. Anthropic ships no local
tokenizer, so Claude Code items can only be estimated — while the per-turn total
is exact. Calibration reconciles the two: estimates are scaled to fit the known
total, and whatever cannot be attributed becomes an explicit residual row rather
than being smeared across the visible categories.

```
Context — 146,820 tokens  [observed, exact]

  Tool outputs      61,240  41.7%  [calibrated]
  Conversation      37,820  25.8%  [calibrated]
  Instructions      19,110  13.0%  [calibrated]
  Repo context      14,420   9.8%  [calibrated]
  Unattributed      14,230   9.7%  [residual — system prompt + tool schemas]
```

---

## Building

Requires Rust 1.85+. The toolchain is pinned in `rust-toolchain.toml` to
`stable-x86_64-pc-windows-gnu`, because this development machine has no MSVC
"C++ build tools" workload installed.

```bash
cargo build --workspace
cargo test --workspace
```

Dependencies are kept deliberately few — `walkdir` and `clap`'s default features
were both dropped to avoid `windows-sys`, which needs mingw's `dlltool` on
`PATH` and which this project does not otherwise need. Fewer crates also means a
cheaper audit of the "nothing leaves this machine" claim.

> Before starting the Tauri desktop shell, install the Visual Studio C++ build
> tools and switch the toolchain to `stable-x86_64-pc-windows-msvc` —
> Tauri/WebView2 on Windows is far better trodden on MSVC.

---

## Current state

| Component | Status |
|---|---|
| `ct-domain` — model, ports, calibration | Implemented, 29 tests |
| `ct-adapters` — JSONL reader, tokenizers, raw source, directory walk | Implemented |
| `ct-adapters` — Codex ACL (parse + replay reconstruction) | Implemented |
| `ct-adapters` — Claude Code ACL | Documented, not implemented |
| `ct-application` — use cases | Not started |
| `ct-cli` — `sessions`/`inspect`/`context`/`largest`/`doctor` | Not started |

Planned CLI surface, once the core is proven:

```
ct sessions
ct inspect <id>
ct context <id> --turn 42
ct largest <id> --turn 42
ct diff    <id> --turn 20..42
ct doctor  <id>
```

`--json` on every command from the start, so ContextTrace is pipeable into other
tooling before any desktop UI exists.

---

## Testing approach

Committed fixtures are **hand-authored synthetic JSONL** covering each event
shape found in the format probe, including a deliberate unknown-event-type case
asserting graceful degradation. Real session logs are never committed; they are
used only as a local, gitignored corpus for a zero-panic smoke test that also
reports a histogram of unrecognised event types — which is how a format change
upstream surfaces as a count rather than a crash.
