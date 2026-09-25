# On-disk formats

ContextTrace reads persisted Codex CLI and Claude Code session JSONL. The
descriptions here summarize the shapes supported by the committed fixtures and
compatibility catalog; they do not promise support for every producer version
or for live stdout/app-server streams. Examples and fixtures are synthetic or
redacted, not copied from a private session corpus.

Back to the [README](../README.md) · see also
[compatibility](FORMAT-COMPATIBILITY.md), [methodology](methodology.md) and
[architecture](architecture.md).

## Codex CLI

Session records use an outer envelope with a `type` and `payload`. Persisted
`response_item` records describe request items; session metadata and turn
context provide additional state. A `compacted` record may contain
`replacement_history`, which records the replacement item list and supports an
exact structural comparison when that evidence is available.

Some payloads are encrypted, structured or image-bearing rather than ordinary
text. Tokenizing a JSON serialization or encoded image would not measure what
the model saw, so those items are not presented as exact text-token counts.

## Claude Code

Assistant records can contain a `usage` object with input, cache-creation and
cache-read token fields. The prompt total is derived from the applicable
fields; reading only `input_tokens` can undercount when cached input is
present.

Events are connected by `parentUuid`, forming a **DAG** rather than a simple
conversation list. Rewinds or edits can create branches. One API response may
span several records sharing a `requestId`. `attachment` records can describe
injected context and its origin.

Some responses contain multiple API iterations and aggregate usage fields.
Treating aggregate usage as one request can produce an impossible prompt size;
ContextTrace uses the available call structure and reports affected turns.
Reasoning text may be redacted while an opaque signature remains, and a tool
result persisted to disk may be larger than the content that was sent to the
model. Such records require conservative size estimates and explicit
measurement limits.
