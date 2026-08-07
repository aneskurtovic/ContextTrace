# Methodology

How ContextTrace turns two agents' logs into comparable numbers, what those
numbers can and cannot support, and the cases where it refuses to answer.

Back to the [README](../README.md) · see also [formats](./formats.md) and
[architecture](./architecture.md).

## The asymmetry that shapes everything

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

**Oversized outputs do not disappear to preserve that bound.** Codex response
items above 4 MiB are not materialised as JSON trees. A narrow lexical scan
recovers their tool-call link and measures only decoded text or structured
output with inline `image_url` values removed. The item remains visible with
observed membership and an estimated size, its label states how many image
payload characters were excluded, and no visual-token charge is assigned a
base64-derived text estimate during observed-total reconciliation. Exact mode
refuses these lines from their recorded size before fetching them.

The ordinary (sub-4 MiB) and oversized parsers now use the same policy:
inline data-image payloads are excluded from the text proxy and reported as an
image count plus excluded payload characters. Exact mode refuses image-bearing
items rather than presenting base64 to the tokenizer. Fixtures cover parsed
JSON, escaped data URLs, exactly 4 MiB and one byte above the parse cap.

**It is far cheaper than the design implied.** `SourceRef` seeks to a byte
offset, so this is one seek per item, not a scan. On the largest local Codex
session (94.6 MB, 269 items at the peak turn) `ct context` takes 1.34 s and
`ct context --exact` takes 1.49 s in a release build.

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

## The ratio is measured, not assumed

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
Context at turn 104 - 419,905 [observed]

  Tool outputs              265,391   63.2%  █████████████·······  [estimated]
  Reasoning                  50,451   12.0%  ██··················  [estimated]
  Tool calls                 49,431   11.8%  ██··················  [estimated]
  ...

  Ratio      2.39 characters per token, measured from this session's own
             usage across 66 turn pairs (spread 2.0x).
  Unlogged   ~37,143 tokens the agent never wrote down -- its system prompt
             and tool JSON schemas. Measured, not assumed.
```

That figure is corroborated independently: at turn 1 of a session, where the
cache is cold and the arithmetic needs no fitting at all, the gap between logged
content and reported prompt is ~40,000 tokens.

## When it does not work, it says so

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

The same rule decides which turn a command defaults to. `ct context <id>` with no
`--turn` picks the session's largest, but "largest" needs sizes, and **256 of the
775 local sessions have turns with no usable size** — subagent transcripts, which
log a turn without filling in its usage. Ranking those on `unwrap_or(0)` gives
every turn the same key and silently returns the last one, so the command used to
answer:

```
Context at turn 1 - 0 [observed]
  Developer instructions          0    0.0%  ····················  [estimated]
  Calibration: estimates scaled by 0.00 to meet the observed total of 0.
```

An empty context, stated as **observed**, for a turn holding about 10,400 tokens.
It now refuses and names the way out, and that way out is honest about what it
is:

```
$ ct context agent-a168…
error: no turn in this session reported a usable prompt size, so there is no
largest turn to default to -- pass --turn N to inspect one (1 turn(s) available)

$ ct context agent-a168… --turn 1
Context at turn 1 - 10,372 [estimated]
```

Worth noting how that defect survived: both halves were individually well-typed.
An all-zero `usage` object read as `Some(0)` is a plausible reading, and
calibrating estimates to an observed total is exactly what the tool should do.
The false claim only appeared when they composed. Sum types make a bad value hard
to *write*; they do not make a bad value hard to *derive*.

## Reading the numbers

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
