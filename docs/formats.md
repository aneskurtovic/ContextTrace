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

The 0.161.0 schema contract additionally covers structured `tool_search_call`
arguments, legacy `local_shell_call`, `additional_tools`, the
`compaction_summary` alias and `configuration_update` controls. Known
`retained_context` families are model-invisible host evidence and do not add
another copy of their text to context. Audio URLs, file-based images and nested
encrypted content remain opaque estimates. See the
[dated audit](FORMAT-AUDIT-2026-10-07.md) for unhandled variants and evidence limits.

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

Malformed JSON and skipped oversized Claude records are unrecognised events,
so diagnostics expose the lost information instead of treating it as harmless
session metadata. Parsing continues with subsequent records.
