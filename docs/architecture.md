# Architecture

How the code is arranged, which rules the type system enforces rather than
documents, and how the whole thing is tested.

Back to the [README](../README.md) · see also [formats](./formats.md) and
[methodology](./methodology.md).

## Two principles that shape the code

**Observed vs reconstructed.** Agent logs are not a perfect record of the API
request. Every figure carries its provenance, and the distinction is enforced by
the type system rather than by documentation — see
[`TokenCount`](../crates/ct-domain/src/model/tokens.rs), whose variants presentation
code must match on before it can extract a number.

**Adapter-based.** Agent formats are foreign models behind anti-corruption
layers. Adding Cursor, Gemini CLI or OpenCode means implementing one trait and
adding one line to the composition root.

## Ports and adapters

Ports and adapters, with the dependency rule enforced by Cargo rather than by
convention.

```
             ct-cli              ct-ui
                 \                /
                  \  ct-runtime  /
                   /            \
      ct-application            ct-adapters
                   \            /
                    ct-domain
```

Every arrow points inward. `ct-domain` depends on nothing but `serde` and
`chrono`. `ct-adapters` implements ports declared in `ct-domain` and never calls
into `ct-application`. `ct-runtime` is the shared composition root that selects
concrete adapters and tokenizers once for both driving interfaces.

| Crate | Responsibility |
|---|---|
| `ct-domain` | Entities, value objects, the `AgentSession` and `ContextSnapshot` aggregates, domain services, port traits. No I/O. |
| `ct-application` | Use cases orchestrating domain services over ports. |
| `ct-adapters` | Driven adapters: per-agent ACLs, token estimators, filesystem raw-event source. |
| `ct-runtime` | Shared composition root used by CLI and desktop. |
| `ct-cli` | The `ct` binary and terminal presentation. |
| `ct-ui` | Tauri v2 desktop driving adapter and React interface. |

The analysis crates and CLI keep a deliberately small dependency surface.
Tauri necessarily adds the native window/webview stack; it is isolated in
`ct-ui`, and both interfaces share their concrete adapter and tokenizer choices
through `ct-runtime`.

## Invariants worth knowing

**Confidence never launders upward.** Combining an observed fact with an
estimated one yields an estimate. There is no path by which a guess becomes a
measurement.

**A context snapshot always adds up.** `ContextSnapshot` can only be built with
item tokens plus the unattributed residual equalling the reported total. An
inconsistent breakdown is unrepresentable, not merely discouraged.

**Exact duplicate content is named and costed.** On `ct context`'s opt-in
analysis path, each adapter reduces model-visible content to a fixed-size
identity, excluding retry-specific transport ids. Other commands do not pay to
analyse content they never compare. `ct context` groups equal identities,
reports the tokens occupied by every copy and the avoidable tokens after the
first, and includes all groups in `--json`. Content the log hides — notably
Claude Code's redacted thinking — is not fingerprinted, because an exact-match
claim cannot be made from it.

**Low-information blocks are ranked, not called removable.** The same opt-in
pass records each visible payload's DEFLATE size without retaining its content.
`ct context` ranks large, unusually compressible items by
`tokens × (1 − compressed/original)`, shows the ten strongest findings and
includes all of them in `--json`. The number is explicitly a waste *score*:
compression reveals repetition, but cannot prove which repeated structure the
model did not need.

## Component status

| Component | Status |
|---|---|
| `ct-domain` — model, ports, calibration, filtering | Implemented |
| `ct-adapters` — Codex ACL, Claude Code ACL, tokenizers, raw source, tool targets | Implemented |
| `ct-application` — use cases, diagnostics, secret scan/redaction, NDJSON export, item lifecycle, diff, growth | Implemented |
| `ct-runtime` — shared CLI/desktop composition root | Implemented |
| `ct-cli` — the thirteen commands below | Implemented |
| `ct-ui` — Tauri v2 + React paged search, growth, composition, contributor lifecycle and Context Doctor | Desktop MVP accepted: real-corpus installed search/filtering and native 1024×680/1440×900 checks; new panels pass responsive acceptance |
| Standalone JSONL fixture files | Implemented |
| Reproducible CI, installable release artifacts, release documentation | Windows CI is green; unsigned NSIS/CLI/checksum draft packaging implemented |
| Session metadata search | Implemented server-side with explicit paging |
| SQLite index | Deferred until measured desktop performance requires it |

`cargo fmt --check` clean, `clippy` clean at zero warnings, the React
production bundle and Tauri command bridge build, the release CLI answers
`ct --help`, and `ct doctor --dir` recognises every event type across the
current local corpus. The Windows CI workflow enforces these gates on stable
and Rust 1.88. Work is queued in [BACKLOG.md](../BACKLOG.md), which is the
authoritative list. [IDEAS.md](../IDEAS.md) is an idea pool and nothing in it
is scheduled until it is pulled in there with a `CT-nnn` id.

## Fixtures

Committed fixtures are hand-authored synthetic sessions, never captured, each
encoding one way the real formats mislead a reader: a rewound branch that must
not appear in a reconstruction, a compaction boundary the walk must stop at, one
response split across lines under a shared `requestId`, a turn whose cache
figures are the sum of several API calls, a thinking block stripped to its
signature, a tool result whose full output went to disk instead of the model, and
an event type from the future.

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
