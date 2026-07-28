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
> raw JSONL, and a before/after mtime check confirms nothing is written.
>
> A full sweep of all 770 — not just the largest 150 — reports **100%
> event-recognition fidelity** in 2.8 seconds. It did not at first: it found
> three unrecognised types, which is what `ct doctor --dir` exists for, and both
> underlying defects are now fixed (CT-037, CT-038). Most Claude Code
> sessions now report a *measured* figure for the context their agent never
> logged. See [Current state](#current-state).

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

Codex is GPT-family, so `tiktoken` *could* count its items exactly. Anthropic
ships no local tokenizer, so Claude Code items can only be estimated — while the
per-turn total is observed. Calibration reconciles the two: estimates are scaled
to fit the known total, and whatever cannot be attributed becomes an explicit
residual row rather than being smeared across the visible categories.

**By default neither agent's items are counted exactly, including Codex's.**
Both adapters record a character count while parsing and size items from that,
because the lazy-content design exists precisely so that attributing a 99 MB
session does not mean loading 99 MB. So every per-item figure is tagged
`estimated` unless you ask for better.

`ct context --exact` and `ct largest --exact` ask for better, on Codex only.
Each item is seeked to, re-parsed, and measured with `o200k_base` — the encoding
its models actually use, so the result is a measurement rather than a denser
guess. Two things about it are worth knowing before trusting it, and the command
prints both:

**It covers about three items in five.** Across the local corpus, of 20,768
`response_item` lines, 6,091 carry `encrypted_content`, 2,498 carry a structured
`output` object, and 99 carry an inline `image_url`. None of those can be
tokenized honestly — a ciphertext blob is not the text the model read, image
data URLs are charged as patches rather than BPE tokens, and re-serializing a
structured output would measure `serde_json`'s key order rather than Codex's.
Those items keep their character estimate, and the header says how many did.

**It is far cheaper than the design implied.** `SourceRef` seeks to a byte
offset, so this is one seek per item, not a scan. On the largest local Codex
session (99 MB, 262 items at the peak turn) `ct context` takes 0.43 s and
`ct context --exact` takes 0.50 s.

**What it does not make the residual mean.** It is tempting to conclude that
once every item is measured, the remainder is purely context the agent never
logged. It is not. An exact count is the *model-visible text* of an item —
field names, role markers and block structure are excluded on purpose, because
counting serialized JSON is the mistake above. So on a fully-exact turn the
remainder is the tool schemas plus that framing. Measured on two such Codex
turns it came to **10,218 and 9,630 tokens** — 53% and 42% of their prompts.
Large, stable, and now attributable to something specific rather than to our own
arithmetic. `ct context --exact` says exactly that instead of the usual "plus
whatever the estimates missed", which would be false there.

Exactness is deliberately absent from `ct trace` and `ct residual`: both sweep
every turn, so the per-item cost would multiply by turn count, and `trace`
answers a membership question that does not depend on the estimator at all.

Claude Code refuses `--exact` outright, with an error rather than a footnote.
Re-reading its text would buy a slower estimate and nothing else, and handing
back estimates under a flag named `--exact` is the exact failure this project
cannot afford.

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
| `ct-domain` — model, ports, calibration, filtering | Implemented, 50 tests |
| `ct-adapters` — Codex ACL, Claude Code ACL, tokenizers, raw source, tool targets | Implemented, 101 tests |
| `ct-application` — use cases, diagnostics, drift sweep, NDJSON export, item lifecycle, diff | Implemented, 49 tests |
| `ct-cli` — the ten commands below | Implemented, 23 tests |
| Standalone JSONL fixture files | Implemented, 13 tests |
| Context-growth timeline, search, SQLite index, desktop shell | Not started |

**236 tests** passing, `clippy` clean at zero warnings, and `ct doctor --dir`
recognises every event type across the whole local corpus. Work is queued in
[BACKLOG.md](BACKLOG.md), which is the authoritative list: 26 done, 1 next, 11
todo, 2 deliberately dropped. [IDEAS.md](IDEAS.md) is an idea pool and nothing
in it is scheduled until it is pulled in there with a `CT-nnn` id.

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
ct context <id> [--turn N] [--exact] [filters]  # defaults to the largest turn
ct largest <id> [--turn N] [--limit] [--exact] [filters]
ct trace   <id> --item <id-or-label>   # one item's lifecycle across the session
ct residual <id> [--from N] [--to N]
ct diff    <id>[@turn] <id>[@turn]     # or A..B; each side defaults to its peak
ct doctor  <id>
ct doctor  --dir [PATH]                # sweep for format drift; exits 1 on any
ct export  <id>                        # the whole session as NDJSON, streamed

filters: --source <kind[:text]>  --category <name>
         --confidence <level>    --min-tokens <n>
```

### Getting the numbers out

`ct export <id>` streams the whole session as NDJSON — one record per line,
externally tagged, with a `schema` on the header line:

```
{"type":"session","schema":1,"id":"019f8f07…","agent":"codex","turns":1274,
 "estimator":"o200k_base","fidelity":1.0}
{"type":"turn","turn":1,"total_tokens":13416,"accounted_tokens":13416,"residual_tokens":0,…}
{"type":"item","turn":1,"id":"codex:1","category":"system-instructions",
 "label":"Codex system prompt","tokens":4666,"confidence":"estimated","line_no":1}
```

`duckdb` reads NDJSON natively, so this replaces the dropped DuckDB export
(CT-032) without a database driver in a dependency tree whose auditability is
the privacy claim:

```sql
SELECT category, sum(tokens) FROM 'session.ndjson'
WHERE type = 'item' AND turn = 88 GROUP BY 1 ORDER BY 2 DESC;
```

**The residual is an item row, and that is the whole design.** A consumer's
first query is `sum(tokens) GROUP BY turn`. If only real items are emitted, that
sum silently disagrees with the prompt size the agent reported — on the worst
local turn by 53% of the context. So the remainder is a row of its own, *and*
the turn record carries the totals, so the naive query is correct and the two
ways of asking cross-check each other. Verified across the largest multi-turn
session: 1,274 turns, 362,218 records, **zero turns where the item rows failed
to sum to the reported total**.

Every figure carries its `confidence`, and a calibrated one keeps its
`raw_estimate` — an export is the easiest place to lose the guarantee the type
system enforces inside the process. The header names the estimator, because for
Claude Code the ratio is fitted per session (CT-014) and a file whose numbers
cannot be reproduced is a file whose numbers cannot be trusted. Message and
tool-output previews are excluded: labels carry the paths and commands that make
a size analysable, conversation content stays in the session file until
redaction exists (CT-025).

It streams, and the cost is **turns, not bytes** — every turn is reconstructed,
so the 99 MB session with 88 turns exports in 0.46 s while the 25 MB one with
1,274 turns takes 2.1 s.

### Catching an agent that changed its format

Both agents evolve their log formats, and a type this build has not learned
degrades every other command quietly — the events still parse, they just stop
being attributed. `ct doctor --dir` parses every discovered session and reports
what was not understood, exiting non-zero if anything was:

```
$ ct doctor --dir          # the first run, before CT-037 and CT-038
Format drift sweep

Sessions   774 scanned
           711 claude-code
           63 codex
Events     126,330 parsed

Unrecognised event types (99.60% fidelity)

AGENT           EVENTS   SESSIONS  TYPE
claude-code        369     4/774   started
                                   ct inspect journal --raw
claude-code         97     4/774   result
                                   ct inspect journal --raw
codex               38     3/774   response_item/web_search_call
                                   ct inspect 019f4181-29a7-75d0-b60e-935e615a18f0 --raw
```

Three design points, each one a way this could have been less useful:

**The gate is presence, not a percentage.** A new type appearing once in half a
million events is the same news as one appearing everywhere. A fidelity
threshold would hide exactly the early case worth catching.

**Sessions-per-type is what makes the histogram readable.** A raw count cannot
separate a long-running experiment in one session from a change that has shipped
to all of them.

**`--dir` narrows the discovered sessions rather than walking a directory,** so
each file's agent is known from its descriptor. Detecting an agent from a file's
contents would mean inventing a rule, and a misdetected file reports as
wholesale drift — the loudest possible way for a guess to be wrong.

That is the real first run, and both findings were real. The sweep now reports
`Recognised every event type in every session` across 770 sessions and 126,130
events, because both were fixed:

**Codex `web_search_call` was unparsed** (CT-037) — 38 real API items that
occupied context. The fix is worth a line for what it did *not* need. A web
search carries no tool name and no arguments; what it did lives in `action`,
either `{type: "search", query}` or `{type: "open_page", url}`. The shared key
list that names tool targets already ranks `url` above `query`, so both shapes
named themselves with no new per-tool knowledge:

```
   100    0.2%  Tool calls   web_search site:help.instagram.…conds Instagram best practices
                codex:70  from tool: web_search [estimated]
```

**`journal.jsonl` was being discovered as a session** (CT-038) — workflow
bookkeeping under `subagents/workflows/`, which also put four entries called
`journal` into `ct sessions`. Two obvious rules were wrong, and measuring caught
both: requiring a UUID filename would have discarded 625 of 711 sessions, since
subagent transcripts are named `agent-<hex>.jsonl`; requiring a `uuid` on the
first line would have discarded 90, since that many sessions open with a
`last-prompt` or `mode` sidecar. The rule that survives is stated from what a
session *is* — reconstruction is an ancestor walk over `uuid`-keyed events, so a
file with no `uuid` anywhere has no node the walk could start from. It fails
open when the 1 MiB prelude budget runs out, because discarding a real session
is a worse error than keeping four journals.

Scanning the prelude also fixed a bug it exposed: header fields were read from
line 1 only, so 74 of 707 sessions carried no `started_at` — and `ct sessions
--since` deliberately keeps timestamp-less sessions, so all 74 leaked past every
date filter. Each field now comes from the first line that has it. Missing
`started_at` went 74 → 0, at a cost of 0.3 ms per session at discovery.

### Filtering without lying about the whole

Both context views narrow by provenance and size, which is how you get from "a
turn ballooned" to the specific tool result responsible:

```
$ ct largest 60c7495d --category tool-outputs --min-tokens 2000

Largest context contributors at turn 59 (total 135,668 [observed])
Filter     category=tool-outputs, min-tokens=2000
           4 of 228 items, 27,462 of 135,668 tokens — 20.2% of this turn,
           excluding the unattributed remainder

   14,805   10.9%  Tool outputs   Read C:\Users\anesk\source\repos\VoxMux\BACKLOG.md
    7,822    5.8%  Tool outputs   Read C:\Users\anesk\source\repos\VoxMux\docs\HANDOFF.md
    ...
Shares are of the turn's full 135,668 tokens, so these rows deliberately do not
add up to 100%. 4 of 228 items matched.
```

`--source` matches the origin and, optionally, what it names: `tool`,
`tool:Bash`, `file:schema.ts`, `harness:skill_listing`. `--confidence` is a
floor rather than an exact match, so `derived` admits observed items too.

Filtering is where a tool of this kind most easily starts lying, by recomputing
percentages against the subset so four rows "account for 100% of the context".
They do not — and the other 79% is exactly what the person filtering needs to
keep in view. The filtered view therefore borrows the snapshot rather than
owning the matched items, so the denominator is only reachable through the whole
and there is no subset sum available to divide by. The unfiltered views are the
same code path with an empty filter.

The unattributed remainder is excluded from any filtered view unless asked for
by name (`--category unattributed`): it has no source and no line in any file,
so a query *by provenance* has nothing to match it against.

A filter that matches nothing prints the categories, sources and confidence
levels the turn actually contains, because an unexplained blank is
indistinguishable from a broken flag.

Rows are named by what a call *acted on*, not just which tool ran — the path for
a read, the command for a shell call — taken from the call's own arguments. A
turn holding four `Read` results is otherwise four identical rows with different
numbers. The argument names are tried in order of specificity rather than
hardcoded per tool, so an unfamiliar MCP tool taking a `path` or a `query` is
named correctly anyway; where nothing matches, the bare tool name stands, because
a wrong filename is worse than no filename. There is no `Tool output:` prefix —
the category column beside it already says that, and a label restating its own
column spends a fifth of the width saying nothing.

### One item's lifecycle, and the difference between gone and evicted

`ct context` sees a single turn, so it cannot say how long something has been
sitting in the window. `ct trace` reconstructs every turn and reports where an
item actually appears:

```
$ ct trace 257a927b --item claude:35

Item      claude:35
          Read C:\Users\anesk\source\repos\L…oids\client\e2e\game-a11y.spec.ts
Category  Tool outputs, from tool: Read

Entered   turn 4
Present   turns 4-210  (207 of 360 turns scanned)
Size      8,996 tokens at turn 210 — 2.6% of that turn's 339,687 [estimated]
Left      after turn 210 — the compaction at turn 211 removed it, reclaiming
          326,941 tokens
```

The item reference is an id (shown by `ct largest`) or any part of a label;
labels are not unique, so an ambiguous one lists the candidates with their ids
rather than guessing.

Four things this view refuses to say:

- **Absent is not unknown.** A turn whose reconstruction fails tells us nothing
  about the item, so it ends a run rather than being read across or counted as a
  departure.
- **Gone is not evicted.** Claude Code's log is a DAG. When an item disappears
  with no compaction, the later turns descend from a different branch — the
  conversation was rewound or a message edited — and the item was never in their
  prompts to be evicted from. That case is named as a branch change, and it is
  common: it happens in 47 of 86 recent local sessions.
- **A subagent's turns are not this thread's turns.** A subagent has its own
  context window, so main-thread items are legitimately missing from its turns.
  Counting that as absence would make every long-lived item appear to flicker.
- **The size is one measurement, not a series.** An item's text does not change
  while it sits in context; only the calibration scale moves, so a per-turn size
  column would show movement the item does not have. It is measured at the last
  turn holding the item, while `ct largest` defaults to the session's *peak*
  turn — so the same item reads as 12.3% of 73,138 there and 2.6% of 339,687
  here. Same token count, different denominator, and each line names the turn it
  used.

For Codex the fold only clears at a compaction, so a departure without one is a
defect in ContextTrace rather than a fact about the session — and the view says
exactly that instead of inventing a branch Codex's linear log cannot have. A
sweep of 240 items across 40 local sessions produced no such case.

Building this found one: the Codex system prompt was being dropped at every
compaction, because reconstruction cleared the whole item list.
`base_instructions` is not part of the item list a compaction replaces — Codex
sends its system prompt as the request's own field. Measured across every
compaction in the local Codex corpus (47 events in 15 sessions, 593
`replacement_history` entries: 450 user messages, 96 developer messages, 47
opaque `compaction` blobs), **no entry carries the system role**. So it survives
the fold, and post-compaction turns had been understating their accounted
content and inflating their unattributed remainder by its size.

`ct residual` tracks the context the agent never wrote down, turn by turn. Since
nothing in the log records a tool being registered or an MCP server connecting,
a sustained step in that remainder is the only evidence such a change happened.
It compares the median of the five turns either side rather than adjacent turns,
because the remainder drifts — see [BACKLOG.md](BACKLOG.md) CT-016 for why that
distinction had to be built in.

`--json` on every command, so ContextTrace is pipeable into other tooling before
any desktop UI exists. Domain types serialise as tagged sum types
(`{"kind":"calibrated",…}`), so downstream scripts never regex strings.

### Comparing two turns without comparing two rulers

`ct diff` answers "it worked yesterday and fails today on the same task". Each
side is `<id>[@<turn>]` and defaults to that session's largest turn, which is
always printed rather than assumed — two unstated peaks at turn 5 and turn 400
would make depth read as difference.

```
ct diff 60c7495d@20 60c7495d@59        # two turns of one session
ct diff 257a927b..25e27e70             # two sessions, each at its peak
```

The thing that makes this harder than subtraction: **Claude Code item sizes come
from a characters-per-token ratio fitted to each session's own usage.** Across
the sessions on this machine that ratio runs from 2.00 to 2.55 — a 27% spread.
Two sessions are therefore reported on two differently graduated scales, and a
raw subtraction carries the change in content *and* the difference between the
instruments with no way to tell which is which. The residual is worst affected,
being `observed_total − sum(estimates)`: every token the ratio moves in the items
lands there with the sign flipped.

So the skew is computed and carried to every row as the largest delta the
instruments alone could explain:

```
  Instrument heuristic:chars/2.0 vs heuristic:chars/2.5 -- 20.9% apart.

  CATEGORY                       LEFT       RIGHT       DELTA
  Other                       111,151           4    -111,147  changed
  Tool calls                   29,927      97,908     +67,981  changed
  File contents                 2,155      38,912     +36,757  changed
  Unattributed                 55,366      61,378      +6,012  within +-67,263
```

The last row is the point. Read naively it says this session leaves 6,012 more
tokens unaccounted for; in fact that is under a tenth of what the two fits alone
explain, and nothing may be concluded from it. Note also that the remainder is
bounded by the *accounted* total, not by its own size — a ratio moving the items
by 9% moves the residual by 9% **of the items**. Bounding it like an ordinary row
understated it by more than half, which the test that found it now guards.

Comparing a Codex session with a Claude Code one is a refusal rather than a wider
bound. No factor relates a measured count to a ratio estimate, and Codex logs its
own system prompt, so the two residuals are not even the same quantity. Token
deltas are withheld and the counts — prompt totals from the agents' own usage
records, item counts, tool-call counts — carry the comparison. That is why the
view is ordered by how instrument-free each axis is: a reader who stops after the
header has still read something true.

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
