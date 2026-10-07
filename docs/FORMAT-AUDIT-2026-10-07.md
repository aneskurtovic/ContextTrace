# Codex and Claude JSONL audit, 2026-10-07

## Scope and evidence

This review covers the persisted JSONL adapters, content measurement, event
classification, and fixture provenance. JSONL records have nested fields rather
than fixed columns. Additional fields are tolerated; recognizing an envelope
does not establish that every nested field has been reconstructed.

Installed versions were Codex CLI **0.161.0** and Claude Code **2.1.293**.
The official Codex latest release API and Claude changelog reported the same
versions during this review:

- [Codex 0.161.0 release](https://github.com/openai/codex/releases/tag/rust-v0.161.0)
- [Codex response/content definitions at 0.161.0](https://github.com/openai/codex/blob/rust-v0.161.0/codex-rs/protocol/src/models.rs)
- [Claude Code changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)
- [Claude session storage and resumption](https://code.claude.com/docs/en/sessions)
- [Codex stdout JSONL](https://developers.openai.com/codex/noninteractive/)

A structural sample read the 20 most recently modified JSONL files from each
local producer root. No transcript content or identifiers were copied into this
report. Codex records declared 0.160.0 and 0.161.0. Claude records declared
2.1.282, 2.1.287, 2.1.288, 2.1.289 and 2.1.292; **none declared 2.1.293**.
The sample found no malformed Codex records and one malformed Claude record.
Sampling recent files is not exhaustive coverage of either producer.

## Findings and fixes

| Area | Finding | Change |
|---|---|---|
| Claude malformed/oversized lines | Skipped data became an ordinary session event and could leave fidelity apparently clean. | Classify unreadable records as unrecognised; retain their source position. |
| Codex retained review evidence | A full corpus sweep found a previously unknown `retained_context` record. Upstream defines it as model-invisible host evidence. | Recognize the verified-answer and delivered-assistant-message families as metadata without counting their text twice; unknown families remain unrecognised. |
| Codex tool search | Upstream arguments are a JSON value, while the reader only measured strings; calls without a name appeared as unknown tools. | Measure structured arguments as an opaque serialization proxy and name the tool `tool_search`. |
| Codex legacy shell | Official `local_shell_call` records were unrecognised. | Preserve call identity, command and estimated size. |
| Codex tool definitions | `additional_tools` was unrecognised and its definitions had no measured size. | Classify as a context injection with an explicitly estimated serialization proxy. |
| Codex compaction alias | Upstream accepts `compaction_summary` as an alias for `compaction`. | Recognize both, including oversized records; encrypted content alone does not prove replacement history. |
| Codex backend controls | `configuration_update` was unrecognised. | Recognize as a control record rather than model text. |
| Codex opaque content | Nested encrypted agent content, audio URLs and file-based images were skipped, allowing mixed content to be treated as entirely readable text. | Include opaque proxies and refuse an exact text recount for the whole mixed item. |

The new `upstream-0.161.0.jsonl` fixture is **synthetic**, based on the pinned
official type definitions. Its version label does not certify a captured session
or every feature of that release. Historical fixtures remain intact.

## Coverage and remaining limits

| Surface or capability | Status |
|---|---|
| Persisted message/tool/reasoning records, Claude DAG branches, request usage and compaction | Existing semantic contracts retained; targeted new tests cover the findings above. |
| New fields on otherwise known records | Tolerated; not automatically interpreted or exposed as dedicated UI fields. |
| Unknown outer or response-item types | Retained as unrecognised events; parsing continues. |
| Unknown Codex `event_msg` and Claude `system` subtypes | Generic session events; an unknown subtype can still evade the drift count. |
| Codex `image_generation_call`, `context_compaction` and request-only `compaction_trigger` | No dedicated semantic reconstruction contract in this audit; remain unrecognised if encountered as response items. |
| Audio, encrypted content and image references | Opaque, estimated proxies only; no exact media token measurement. Reference string length is not actual media size. |
| Claude 2.1.293 persisted sessions | Installed/current upstream version verified; a version-matched capture is still needed. |
| Codex 0.161.0 | Observed in local sample plus a new synthetic upstream contract; no claim of exhaustive feature coverage. |
| Codex `exec --json`, app-server and Claude `stream-json` | Separate schemas; not supported by the persisted adapters. |
| Future versions, alternate stores/compression and every historical version | Not certified by this review. |

The shared reader handles LF/CRLF, blank lines, malformed-line recovery and
oversized-line detection. Its 4 MiB limit avoids materializing a JSON tree, but
the raw line is still buffered in memory. Oversized type sniffing is heuristic.
These are explicit limits, not proof of arbitrary hostile-input safety.

Release certification still requires version-matched reviewed captures,
semantic assertions, a version-inventoried corpus sweep and Woodpecker checks.
Local tests are development evidence. No packaging, publication, installer or
updater acceptance follows from this audit.

The initial full corpus sweep read **229 sessions / 99,983 events** (190 Codex,
39 Claude), with no unreadable files. It flagged one malformed Claude record
and one Codex `retained_context` record. This finding led to the additional
retained-context fix above, based on the
[pinned upstream definition](https://github.com/openai/codex/blob/rust-v0.161.0/codex-rs/history/src/retained_context.rs).
An unknown-event count alone does not certify nested-field completeness.

The final rebuilt sweep read **229 sessions / 100,651 events**, with no
unreadable files. The Codex unknown record was resolved. The only remaining
unrecognised event was the malformed Claude line; `doctor` correctly exited
with status 1 rather than suppressing that warning. Counts increased because
the local corpus was still being appended to during the audit.

## Local validation

- Rust workspace: **468 tests passed**, one ignored.
- Rust formatting and workspace Clippy with warnings denied: passed.
- Fixture catalog: **11 fixtures** validated; validator regression checks passed.
- Frontend: **156 tests passed** with one thread and a 15-second test timeout;
  the default fork run failed to start workers, and the first threaded retry
  had one 5-second test timeout. No frontend source changes were made.
- Frontend production build: passed using the existing installed dependencies.

No Woodpecker pipeline was dispatched and no release was created by this work.
