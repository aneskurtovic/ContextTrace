# JSONL format compatibility

ContextTrace supports the persisted, on-disk session formats written by:

- **Codex CLI** under `sessions` and `archived_sessions`;
- **Claude Code** under its `projects` directory.

Those are the product surfaces this repository's adapters parse. They are not
the same thing as a command's machine-readable stdout. Codex `exec --json`,
Codex app-server JSONL, and Claude Code `--output-format stream-json` need
separate adapters and are not included in the persisted-transcript support
claim.

## What â€œsupportedâ€ means

â€œAll versionsâ€ cannot be certified literally: future producers can add event
types or change field meanings. The defensible promise is:

1. preserve backward compatibility for every format family we have retained;
2. keep unknown events readable and never fatal; the diagnostics surface must
   expose a fidelity limit when the parser can prove that an event was not
   understood;
3. verify the latest stable producer before each ContextTrace release;
4. label older and current producer versions with the evidence used; and
5. report prerelease, future, malformed or unverified input as best-effort,
   rather than claiming complete reconstruction.

Agent version, model name, tokenizer support and ContextTrace export schema are
different axes. A log can be parsed even when its model has no exact tokenizer
or price entry. The `producer_version` in the fixture catalog is not a parser
switch and is not a claim that a synthetic fixture was captured from that
release.

## Fixture catalog

[`tests/fixtures/compatibility.json`](../tests/fixtures/compatibility.json) is
the source of truth for the checked-in JSONL templates. Every `*.jsonl` under
`tests/fixtures` must have one entry with:

- the agent and persisted input surface;
- the producer version encoded by the fixture;
- provenance (`synthetic` or an explicitly reviewed local capture); and
- the semantic capabilities the fixture is intended to exercise.

The validator is run in the Windows CI lane:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/check-fixture-manifest.ps1
```

It checks that the catalog and fixture tree agree, every line is valid JSON,
the producer version is present and agrees with the catalog, and every fixture
has a declared capability set. It is a catalog guard, not a substitute for
semantic tests.

The current fixtures are deliberately synthetic and therefore provide
**parser-contract evidence only**. Their embedded version strings must not be
reported as proof that those real releases have been captured or certified.

The adapters are intentionally tolerant of known outer envelopes. For example,
an unrecognised `event_msg` subtype or Claude `system` subtype may still be
classified as a generic session event when its envelope is known. That keeps
the session readable, but it does not always reduce the fidelity score. This is
a known limitation of the current drift signal and is why semantic fixture
tests, versioned captures and manual release review remain necessary.

## Update procedure

When a Codex or Claude release changes its persisted transcript shape:

1. copy a redacted, minimal reproducer into the appropriate fixture directory;
2. add or update its catalog entry and name the observed producer version;
3. add a semantic assertion for the changed event or field, including the
   expected reconstruction and measurement confidence;
4. retain the older fixture so the previous format family remains covered;
5. run `ct doctor --dir --json` against an isolated, version-inventoried local
   corpus and record unknown event types and unreadable files; and
6. update this catalog's review date and the release notes with the exact
   producer versions and surfaces verified.

Never commit a real session merely to refresh a template. Prompts, source code,
tool output and credentials can be present even after a superficial scrub.
Use hand-authored fixtures or a reviewed redaction process, and keep the real
corpus local.

## Release support matrix

| Input surface | Current commitment |
|---|---|
| Codex persisted rollout JSONL | Supported by the Codex adapter and fixture contracts; latest-version certification is a release gate. |
| Claude Code persisted transcript JSONL | Supported by the Claude adapter and fixture contracts; latest-version certification is a release gate. |
| Known historical format families | Retained and regression-tested when a fixture exists. |
| Unknown/future producer versions | Best-effort parse with visible fidelity limits; no completeness guarantee. |
| Codex stdout/app-server JSONL | Not the persisted-transcript adapter surface. |
| Claude `stream-json` stdout | Not the persisted-transcript adapter surface. |

