# Guide

Worked examples, one per command, each showing real output and what the numbers
in it mean. For how those numbers are derived see
[methodology](./methodology.md); for the on-disk formats behind them see
[formats](./formats.md).

Back to the [README](../README.md).

## Getting the numbers out

`ct export <id>` streams the whole session as NDJSON — one record per line,
externally tagged, with a `schema` on the header line:

```
{"type":"session","schema":1,"id":"019f8f07…","agent":"codex","turns":1274,
 "redaction":"none","estimator":"o200k_base","fidelity":1.0}
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
a size analysable, while conversation content stays in the session file.
`--redact-secrets` also protects those analytical labels and every other
string-bearing field — including item ids and structured source paths — with
markers such as `[REDACTED:github-token]`. The header records whether redaction
was requested, and the terminal reports how many exported occurrences changed.

It streams, and the cost is **turns, not bytes** — every turn is reconstructed,
so the 99 MB session with 88 turns exports in 0.46 s while the 25 MB one with
1,274 turns takes 2.1 s.

## Finding credentials without disclosing them again

`ct secrets <id>` re-reads only context-bearing session records and reports a
credential type, turn, line and event type. It recognises provider-shaped
OpenAI, Anthropic, GitHub, GitLab, npm, AWS, Google, Slack and Stripe
credentials, bearer tokens, bare JSON Web Tokens that arrive without a
`Bearer` prefix, PEM private keys, and secret-like assignments in either
syntax a session uses — `API_KEY=…` and the JSON member `"apiKey": "…"` alike,
since names are matched by word rather than by underscore. The recognised PEM
labels are `PRIVATE KEY`, `RSA PRIVATE KEY`, `EC PRIVATE KEY`,
`OPENSSH PRIVATE KEY`, `ENCRYPTED PRIVATE KEY`, `DSA PRIVATE KEY` and
`PGP PRIVATE KEY BLOCK`; a block whose `-----END-----` never arrives is
treated as running to the end of the record, because a truncated record is
exactly where half a key gets written. A bare JWT is recognised by its shape —
three dot-separated base64url segments whose first begins `eyJ`, the base64url
encoding of `{"` that every JWT header starts with — rather than by decoding
and validating it, so an `alg: none` token with an empty signature is not
reported. Credential shapes outside this list, including providers not named
above, pass through unflagged. The matched value is never stored in a finding,
shown in a preview, or made serializable; this is why the command deliberately
has no `--json` mode.

Records are scanned once even when their content survives for hundreds of
turns. Codex replacement histories and the recorded base instructions are
included because they can be placed into a later prompt. Findings are
potential secrets rather than proof of a leak: a valid-looking test fixture is
indistinguishable from a live credential by shape alone, so the location is
reported for the user to inspect in the original local session.

## Catching an agent that changed its format

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

## Filtering without lying about the whole

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

## Exact Codex compaction diffs, without printing prompt content

Codex records the literal `replacement_history` used after a compaction.
`ct compactions <id>` compares that list with the response items immediately
before each boundary and labels every structural item as `dropped`, `preserved`
or `replacement`. The report prints only item type, role, normalized compact
JSON bytes, optional text-token size and source provenance—never prompt, source
or tool-output content. `--json` exposes the same content-free contract.

The operation is deliberately Codex-only. Claude Code does not record a literal
replacement list, so its adapter returns `unsupported` instead of turning an
inference into an exact claim. A missing, malformed or oversized raw line
similarly produces an explicit unavailable result for that boundary.

The first two retained boundaries that unblocked this feature reported 243
dropped / 2 preserved / 5 replacement-only items and 175 / 3 / 1. Every
replacement message at those boundaries structurally matched a prior item; the
replacement-only entries are opaque Codex compaction summaries.

## One item's lifecycle, and the difference between gone and evicted

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
because the remainder drifts — see [BACKLOG.md](../BACKLOG.md) CT-016 for why that
distinction had to be built in.

`--json` on every command keeps ContextTrace pipeable independently of the
desktop app. Domain types serialise as tagged sum types
(`{"kind":"calibrated",…}`), so downstream scripts never regex strings.

## Comparing two turns without comparing two rulers

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
the sessions on this machine that ratio runs from 1.85 to 2.51 — a 36% spread
(14 sessions sampled; an earlier 5-session sample gave 2.00–2.55, so widening the
sample widened the problem rather than averaging it away).
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

## The whole session at once, using only what the agent reported

`ct diff` compares two turns. `ct growth` declines to pick one:

```
$ ct growth 25e27e70
Context growth  25e27e70-…  claude-code
  Turns      889, 4 of which the agent recorded no size for and are drawn as gaps
  Peak       383,810 tokens at turn 778

  ▃▄▅▅▆▇▇▇▃▄▄▄▅▅▅▆▆▆▇▇▇▃▃▄▄▄▅▅▆▆▄▄▄▄▅▅▂▃▃▃▄▄▅▅▆▆▆▇▇███▃▄▄▄▅▅▆▂
         c             c       c     cc              c       c
  turn 1                                              turn 889

  Each column is 15 turns, drawn at the largest prompt among them
  against a zero baseline. A fall within a column does not show.
  'c' marks a column containing a compaction; there are 7.

Largest changes
  turn 779       -324,720  383,810 -> 59,090
  turn 115       -268,935  333,079 -> 64,144
  turn 886       -184,590  243,082 -> 58,492  (across 1 unrecorded turn(s))
```

This is the one command that touches no estimator, no calibration and no
reconstruction. It reads `session.turns()` and nothing else, so every number on
the chart is `Observed` — which is also why 889 turns render instantly.

The caption is doing real work. At 60 columns each column is 15 turns drawn at
their maximum, so **a fall inside a column does not appear at all**; a sparkline
that aggregates silently invites reading a smooth line as a smooth session. The
`c` row marks columns containing a compaction rather than pretending to mark
turns, and a compaction the agent never placed on a turn is counted as unplaced
instead of being attached to a plausible neighbour.

Charting a whole session is also how artefacts become visible, because a single
turn has nothing to look wrong against. This view found two vertical falls to the
floor whose turns carried `{input: 0, cache_creation: 0, cache_read: 0}` — a
usage record written but never filled in, always just before a cache reset. Every
request carries a prompt and a system prompt alone puts the floor in the
thousands, so a zero there is an absent measurement, not a measured absence.
Counted across the whole corpus — not the 60-session sample this was first
quoted from — there are **393 such turn records out of 52,156, in 320 of 775
sessions**, in both agents. Read naively they invented a fall and a matching rise
of ~288,000 tokens that took three of the five largest changes above. They are now gaps, counted in the header
and named when a reported change spans one.

The tempting follow-on — "so the ratio fit was poisoned too" — was measured
rather than asserted, and it is false. Such a turn does enter the fit as a
sample, but both pairs it forms are already rejected by guards written for other
reasons, and its pull on the overhead constant is absorbed by a median. Across
the five affected sessions in a 14-session sample the derived ratio and overhead
are identical with the fix on and off. The fix is real; its reach is per-turn
presentation, not the fit.
