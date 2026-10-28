# JSONL format compatibility

The [2026-10-07 audit](FORMAT-AUDIT-2026-10-07.md) records producer
versions as of that audit, parser fixes, sampled corpus evidence and remaining coverage limits.

ContextTrace supports the persisted, on-disk session formats written by:

- **Codex CLI** under `sessions` and `archived_sessions`;
- **Claude Code** under its `projects` directory.

Those are the product surfaces this repository's adapters parse. They are not
the same thing as a command's machine-readable stdout. Codex `exec --json`,
Codex app-server JSONL, and Claude Code `--output-format stream-json` need
separate adapters and are not included in the persisted-transcript support
claim.

## What “supported” means

“All versions” cannot be certified literally: future producers can add event
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
**parser-contract evidence only**, except for the explicitly marked
`reviewed-local-capture` entries. Those entries are minimal, redacted shape
captures derived from real local JSONL and contain no prompts, source, tool
output, identifiers or credentials.

The 2026-10-07 evidence adds a synthetic upstream contract for Codex **0.161.0**
and a local corpus sweep of 229 sessions / 100,651 events. The only remaining
unrecognised event was one malformed Claude line. Recent Codex files included
0.161.0; recent Claude files reached 2.1.292. The [2026-10-08 follow-up](FORMAT-FOLLOWUP-2026-10-08.md) subsequently added
reviewed persisted/stdout captures for Codex **0.161.0** and Claude **2.1.293**.
Rare Codex shapes still lack feature-matched real captures.

Upstream was rechecked on 2026-10-08 for v0.1.13: the
[Codex stable release](https://github.com/openai/codex/releases/tag/rust-v0.161.0)
is still **0.161.0**; the [Claude changelog](https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md)
now lists **2.1.294**. No version-matched 2.1.294 capture or real-agent resume
acceptance was added in this patch; it remains unverified. These observations
do not certify every feature of either producer.

The historical reviewed-capture evidence is:

- Codex CLI **0.156.1**: a redacted persisted-rollout shape capture covering
  `token_usage_record` and `response_item/compaction`.
- Claude Code **2.1.282**: a real persisted `sdk-cli` transcript capture,
  redacted to its structural shape, including the new queue/title/attachment
  envelope, `atis-latch`, assistant API-error and `cost-state` records.
- Claude Code **2.1.268** remains as a historical local capture covering the
  artifact-ledger sidecars.

The versions listed above are backed by reviewed persisted captures; they are
historical evidence, not the latest installed versions. A release
may claim a producer version only when a redacted persisted capture or an
equivalent reviewed local corpus is available.

Unknown Codex `event_msg` and Claude `system` subtypes produce fidelity
warnings even when their outer envelopes are known. Only explicitly supported
subtypes are classified as session metadata.

### Claude presentation notices (v0.1.10)

`system/away_summary` and `system/bridge_status` are now recognised as session
events. The former is a user-facing recap; the latter is a remote-control
notice. Original byte ranges and adapter text extraction are preserved for
raw inspection. The conversation view continues to include model-context
records and compactions. Neither notice contributes model-context tokens,
starts a model request, or establishes a compaction boundary.

The minimal reviewed fixture contains redacted, nonadjacent records from one
Claude **2.1.239** persisted transcript; parent links to omitted records remain
external. A separate synthetic-chain regression verifies that replay traverses
these notices without dropping the original prompt or counting notice text.
Unknown future subtypes and malformed JSON still warn.

Classification was checked against the installed Claude **2.1.294** executable
(SHA-256 `1f6471eb5a1c21a1f8b54a7827329d64433424dcce51717a54e837adb542163a`).
Its `j6o` and `$6o` constructors create these presentation records; `S_` excludes
system records other than `local_command`, and the request converter skips
them. This is local producer-code evidence, not a complete version certification.
The fixture catalog records the persisted capture's own producer version.

The updated development CLI scanned 43 local Claude sessions / 21,574 events:
one unrecognised event (the existing malformed JSON line), no unreadable files,
and no remaining `away_summary` or `bridge_status` warnings. Doctor still exits
1 for the malformed record. This is local parser verification; the change is
included in v0.1.10 and was absent from v0.1.9.

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
| Codex persisted rollout JSONL | Supported by the Codex adapter and fixture contracts; version-specific claims require reviewed capture evidence. |
| Claude Code persisted transcript JSONL | Supported by the Claude adapter and fixture contracts; version-specific claims require reviewed capture evidence. |
| Known historical format families | Retained and regression-tested when a fixture exists. |
| Unknown/future producer versions | Best-effort parse with visible fidelity limits; no completeness guarantee. |
| Codex stdout/app-server JSONL | Separate explicit timeline importer; no context reconstruction. See [stream imports](STREAM-IMPORTS.md). |
| Claude `stream-json` stdout | Separate explicit timeline importer; no context reconstruction. |


The [audit follow-up](FORMAT-FOLLOWUP-2026-10-08.md) adds reviewed persisted
Codex 0.161.0 and Claude 2.1.293 captures, bounded input handling, nested drift
diagnostics, and separately catalogued stream contracts. Rare new Codex shapes
remain synthetic-only evidence.
