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
analysis path — opt-in in that only `ct context` triggers this load path, so
every other command never pays for it, not that `ct context` itself needs a
flag; its own terminal output prints the findings by default — each adapter
reduces model-visible content to a fixed-size identity, excluding
retry-specific transport ids. Other commands do not pay to analyse content
they never compare. `ct context` groups equal identities,
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
| `ct-cli` — the commands listed in the [README](../README.md#commands) | Implemented |
| `ct-ui` — Tauri v2 + React paged search, growth, composition, contributor lifecycle and Context Doctor | Implemented; 0.1.2 startup passed on two PCs, and 0.1.3 quiet updater behavior passed on a separate laptop; full installer/update acceptance remains open |
| Standalone JSONL fixture files | Implemented |
| Reproducible CI, installable release artifacts, release documentation | Woodpecker CI and packaging workflows are configured; 0.1.3 Windows assets are published |
| Session metadata search | Implemented server-side with explicit paging |
| SQLite index | Deferred until measured desktop performance requires it |

Formatting, Rust lint/tests, frontend tests/build and the fixture compatibility
validators are the repeatable local gates. CI runs portable checks on Linux and
the native Tauri/MSVC build plus Windows-specific CLI smokes on Windows. See
[CI](CI.md) for the current commands and the limits of what those checks prove.
The [roadmap](ROADMAP.md) records non-committal future areas; current release
gates are in [MVP status](MVP-STATUS.md).

## Fixtures

The fixture set combines synthetic parser contracts with explicitly catalogued,
minimal redacted captures. The compatibility manifest records each fixture's
producer version, provenance and semantic capabilities. It contains no
unredacted prompts, source, tool output, identifiers or credentials.

## Testing approach

Fixtures include a deliberate unknown-event case to assert graceful
degradation. Private session corpora are never committed; optional local
format-drift sweeps run against user-selected data and are not part of the CI
fixture corpus. The fixture catalog is the reproducible public compatibility
evidence; it is not an automatic capture of upstream agent releases.
