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

> **Status: milestone 1 complete.** Both adapters, the reconstruction engine and
> the CLI work end to end against real sessions. Verified on a local corpus of
> 774 sessions: the **150 largest — 126 Claude Code and 24 Codex — parse with
> zero failures** and 100% event-recognition fidelity, reported totals match the
> raw JSONL, and a before/after mtime check confirms nothing is written. Most
> Claude Code sessions now report a *measured* figure for the context their agent
> never logged. See [Current state](#current-state).

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

`usage` on each assistant message gives the prompt size as
`input_tokens + cache_creation_input_tokens + cache_read_input_tokens`. Reading
only `input_tokens` is the easiest way to be badly wrong — on a warm cache it
reads `2` for a turn carrying 280,000 tokens.

Events form a **DAG** via `parentUuid`; line order is not conversation order.
One API response spans several lines sharing a `requestId`. `attachment` lines
label injected context with its origin, making instruction provenance observed
data rather than inference.

**Three things in this format will silently corrupt a naive reading.** All three
were found by measuring the corpus, not by reading documentation:

*That sum is not always one prompt.* Some responses carry an `iterations` array —
several API calls behind a single assistant message — and the top-level
`cache_creation_input_tokens` and `cache_read_input_tokens` are the **sums across
those calls**. This holds on 466 of 474 multi-iteration records. Adding them then
yields a prompt no context window could hold: one record reports 844,611 tokens
whose largest actual call was 429,328. ContextTrace reads the largest single call
and `ct doctor` names the affected turns.

*Thinking text is redacted.* 5,820 of 5,869 extended-thinking blocks (99.2%) are
written with an empty `thinking` field and only an opaque `signature` — 27
million characters of signature corpus-wide. That reasoning still occupied the
model's context, so counting it as zero drops the largest single category of
unlogged content.

Size is derived from signature length, but *not* from the obvious statistic. The
median of `signature / thinking` across the 49 blocks that kept both is 2.09 —
and applying it here would be wrong, because those ratios are strongly
size-dependent (6.07 at 60 characters of thinking, 2.49 at 5,957) and the sample's
typical block is 3.7× smaller than the redacted blocks it would be applied to.
Regressing signature on thinking length instead gives slope **2.353**, intercept
−175, R² 0.9707. Using the median would have inflated every redacted block by
about 12%.

*Not everything logged was sent.* A large tool result is persisted to disk and
only a truncated form appears in `message.content`; the full `toolUseResult` was
never in the prompt. Counting the wrong one inflates the biggest category there
is.

### The asymmetry that shapes everything

Codex is GPT-family, so `tiktoken` can count exactly. Anthropic ships no local
tokenizer, so Claude Code items can only be estimated — while the per-turn total
is observed. Calibration reconciles the two: estimates are scaled to fit the
known total, and whatever cannot be attributed becomes an explicit residual row
rather than being smeared across the visible categories.

### The ratio is measured, not assumed

A hardcoded characters-per-token constant is a guess, and it is wrong by
different amounts in different sessions: across the corpus the true figure runs
from **1.49 to 3.49**, because a session of English design discussion and a
session of Windows paths and minified JSON do not tokenize alike.

It would be easy to assume this does not matter, since calibration rescales
everything to the observed total and a uniformly wrong ratio cancels out of the
proportions. It does not cancel out of the **residual** — and the residual is the
whole point, because it is the context the agent never logged.

So ContextTrace derives the ratio from each session's own usage figures.
Differencing consecutive turns cancels the unknown constant, leaving
`Δtokens ≈ Δchars / ratio`; the median of those per-pair ratios resists the one
anomalous turn. Only then is the constant recovered from the levels. That order
matters: solving for the constant first makes the two chase each other.

The payoff is that "what the agent never wrote down" becomes a measurement. Most
swept sessions now report a figure where previously none could:

```
Context at turn 104 — 419,905 tokens  [observed]

  Tool outputs             232,888  55.5%  [estimated]
  File contents             77,734  18.5%  [estimated]
  Reasoning                 26,399   6.3%  [estimated]
  ...

  Ratio      2.42 characters per token, measured from this session's own
             usage across 118 turn pairs (spread 1.9x).
  Unlogged   ~37,143 tokens the agent never wrote down — its system prompt
             and tool JSON schemas. Measured, not assumed.
```

That figure is corroborated independently: at turn 1 of a session, where the
cache is cold and the arithmetic needs no fitting at all, the gap between logged
content and reported prompt is ~40,000 tokens.

### When it does not work, it says so

On roughly one Claude Code session in seven the reconstruction accounts for
**more** content than the prompt held, so the constant comes out negative.

What causes this is **not established**. The leading hypothesis is that Claude
Code removes old content from the context without recording that it has: the
affected sessions have linear chains with no rewinds, yet hold several times more
logged content than their reported prompt. But no marker for such a removal
exists anywhere in the log, and absent one this remains a hypothesis rather than
a finding — so the tool reports the discrepancy rather than modelling a cause it
cannot observe:

```
  Unlogged   not measurable here: reconstruction accounted for more content
             than the reported prompt held, so the hidden remainder cannot be
             separated from the over-count. Treat the rows as proportions.
```

Clamping that to "0 tokens hidden" would turn a broken measurement into a
confident and wrong inventory.

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
| `ct-domain` — model, ports, calibration | Implemented, 30 tests |
| `ct-adapters` — JSONL reader, tokenizers, raw source, directory walk | Implemented |
| `ct-adapters` — Codex ACL (parse + replay reconstruction) | Implemented |
| `ct-adapters` — Claude Code ACL (parse + parent-chain walk) | Implemented, 64 tests |
| `ct-application` — use cases and diagnostics | Implemented, 13 tests |
| `ct-cli` — `roots`/`sessions`/`inspect`/`context`/`largest`/`doctor` | Implemented |
| Standalone JSONL fixture files | Implemented, 13 tests |
| `ct diff`, context-growth timeline, search, SQLite index | Not started |

144 tests passing.

Committed fixtures are hand-authored synthetic sessions, never captured, each
encoding one way the real formats mislead a reader: a rewound branch that must
not appear in a reconstruction, a compaction boundary the walk must stop at, one
response split across lines under a shared `requestId`, a turn whose cache
figures are the sum of several API calls, a thinking block stripped to its
signature, a tool result whose full output went to disk instead of the model, and
an event type from the future.

### Working CLI surface

```
ct roots                          # which local directories are read
ct sessions [--agent] [--project] [--since] [--limit]
ct inspect <id> [--raw] [--limit]
ct context <id> [--turn N]        # defaults to the session's largest turn
ct largest <id> [--turn N] [--limit]
ct doctor  <id>
```

`--json` on every command, so ContextTrace is pipeable into other tooling before
any desktop UI exists. Domain types serialise as tagged sum types
(`{"kind":"calibrated",…}`), so downstream scripts never regex strings.

### A note on reading the numbers

Percentages are shares of an **observed** total, so they are trustworthy.
Individual Claude Code figures are calibrated estimates, and `ct context` prints
both the derived ratio and the scale factor applied.

Two limitations are stated by the tool rather than hidden by it:

- On sessions where reconstruction over-counts, the unlogged remainder cannot be
  separated from the over-count, and `ct context` says so instead of printing a
  zero residual that would imply a complete inventory.
- `ct doctor` reports turns whose figures came from several API calls, and
  reasoning events whose text the log stripped — both cases where a number is
  weaker than its presentation might suggest.

**The main known gap** is the over-counting described above, whose cause is not
yet established. ContextTrace detects the discrepancy and reports it rather than
modelling a behaviour it cannot observe.

---

## Testing approach

Committed fixtures are **hand-authored synthetic JSONL** covering each event
shape found in the format probe, including a deliberate unknown-event-type case
asserting graceful degradation. Real session logs are never committed; they are
used only as a local, gitignored corpus for a zero-panic smoke test that also
reports a histogram of unrecognised event types — which is how a format change
upstream surfaces as a count rather than a crash.

That histogram has already paid for itself twice. It auto-detected five
previously unseen event types (`relocated`, `file-history-delta`,
`custom-title`, `frame-link`, `inter_agent_communication_metadata`) as counts
rather than crashes. And the accuracy defects above — the multi-call sums, the
redacted thinking, the JSON-escaping inflation — were all found by measuring the
corpus against its own reported usage. None of them would have failed a unit
test written from the format alone, which is the argument for keeping a
real-data check in the loop.
