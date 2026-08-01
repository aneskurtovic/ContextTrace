# On-disk formats

What Codex CLI and Claude Code actually write to disk, and the three ways a
naive reading of it goes wrong. Established by probing a local corpus of 709
Claude Code sessions (336 MB) and 61 Codex sessions (191 MB) — not by reading
documentation.

Back to the [README](../README.md) · see also
[methodology](./methodology.md) and [architecture](./architecture.md).

## Codex CLI

`response_item` lines **are** the literal OpenAI Responses API items. Replaying
them reproduces the request body, so context membership is *observed*.
`session_meta.base_instructions` stores the system prompt verbatim. `compacted`
payloads carry `replacement_history` — the post-compaction item list in full —
so discarded content is derivable by diffing rather than merely inferable.

## Claude Code

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
