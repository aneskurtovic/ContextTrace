# Parser audit follow-up

This work follows the [original audit](FORMAT-AUDIT-2026-10-07.md) and the eight
tasks confirmed by the maintainer: the seven handoff tasks plus release work.

## Capture evidence

Installed binaries and upstream release information were rechecked: Codex
0.161.0 and Claude Code 2.1.293. Minimal no-tools requests produced fresh
persisted transcripts and stdout captures. Redaction replaces private text,
paths, identities, and timestamps while preserving schema, linkage and usage.
The persisted fixtures have semantic assertions for declared versions, turns,
usage, event classification and byte-range references. The catalog now has 16
fixtures across five explicitly distinct surfaces.

The available 194-file Codex sessions corpus was structurally searched for
0.161.0 examples of additional tools, structured tool search, legacy local shell,
agent-message, compaction-alias, configuration-update and retained-review
records. None were present in that version's records. A fresh ordinary capture
also did not produce them. Real version-matched evidence for those rare shapes
cannot be supplied from the available captures. They retain synthetic evidence
and are not capture-certified. This remains an evidence limitation, not a claim
that the current producer lacks those capabilities.

Pinned upstream explains one reason a capture cannot be forced by simply
running a prompt: `AdditionalTools` is built in a request-only prefix when the
selected model uses Responses Lite. It is not necessarily written into rollout
history. Rewriting a synthetic record into a captured transcript would not
produce real evidence.

## Replay semantics

Sources are pinned to Codex tag `rust-v0.161.0`, commit
`979011409de0a60b52f179721948e65531d26144`:

- [Response item definitions](https://github.com/openai/codex/blob/979011409de0a60b52f179721948e65531d26144/codex-rs/protocol/src/models.rs)
- [History retention and sizing](https://github.com/openai/codex/blob/979011409de0a60b52f179721948e65531d26144/codex-rs/core/src/context_manager/history.rs)
- [Installation of remote compaction history](https://github.com/openai/codex/blob/979011409de0a60b52f179721948e65531d26144/codex-rs/core/src/compact_remote_v2.rs)
- [Request-only prefix construction](https://github.com/openai/codex/blob/979011409de0a60b52f179721948e65531d26144/codex-rs/core/src/client.rs)

`image_generation_call` is a durable model-generated result. Its revised prompt
is readable text; image result bytes are not a text-token estimate. The result
has a named tool-output event and an explicit unmeasured-media label.

`context_compaction` is a durable opaque history item. It does not carry a list
of evicted messages. The adapter appends the marker without asserting which
history was replaced. A separate `compacted` rollout record establishes the
replacement boundary. A replay regression verifies both behaviors. The
request-only `compaction_trigger` stays unrecognised if encountered as a
persisted response item.

## Drift, media and input bounds

Unknown Codex `event_msg` and Claude `system` subtypes now produce fidelity
warnings. Known telemetry remains non-context metadata. Codex notification
names are pinned to its upstream enum; rollback/raw-response wrappers without
implemented semantics remain warnings.

Image, file-image and audio reference lengths no longer inflate message text
estimates. Mixed media stays ineligible for an exact text recount. Media token
cost is unmeasured and stays unattributed; an estimated text size is not a total
media cost. Encrypted blobs still use an explicit opaque character proxy, not
an exact measurement of their decoded content.

The JSONL reader drains arbitrarily long lines while retaining at most 4 MiB
plus two framing bytes, in addition to its fixed reader buffer and bounded
vector allocation overhead. It records the true line length (saturated at the
domain's u32 limit), subsequent offsets and line numbers. Truncated records
are warnings and are not treated as complete tool results or compaction facts.
Top-level prefix type recovery ignores nested fields and quoted lookalikes.
The raw inspector rejects ranges over 4 MiB before allocation; inspect such
events directly in the original file. These limits apply per record; event
lists and the total parsed session are not constant-memory.

## Separate streaming contracts

The explicit [stream importer](STREAM-IMPORTS.md) has dedicated contracts for
Codex exec stdout, app-server notifications and Claude stdout. It preserves
timeline evidence and usage scope while refusing request-history reconstruction.
It is not a compatibility route through the persisted adapters.

## Release and acceptance

Validation, packaging, publication and uploaded-asset verification must use
Woodpecker. See [RELEASING](RELEASING.md). Local tests are development evidence.
Source commit `193150f0dc1da52a0ba604b152341c1b0e6946f6` passed frontend,
Linux Rust (including Rust 1.88 MSRV), Windows desktop/CLI and smoke checks in
[Woodpecker push pipeline 90](https://ci.aneskurtovic.com/repos/5/pipeline/90).
The initial candidate hit a newer Linux Clippy single-element-loop lint; the
correction is included in the validated release commit.

[Tag pipeline 91](https://ci.aneskurtovic.com/repos/5/pipeline/91) passed its
exact-commit gate and Windows packaging/publication steps. It uploaded and
verified the digests of all six release assets, checked updater metadata
against the staged signature and versioned installer URL, and confirmed the
latest stable release pointer. The GitHub API independently lists
[v0.1.9](https://github.com/aneskurtovic/ContextTrace/releases/tag/v0.1.9) at
that source commit, neither draft nor prerelease, with all six assets uploaded.
Publication time was 2026-10-07 22:25:26 UTC (2026-10-08 in Sarajevo). No
interactive workstation packaging was used as release evidence.

The maintainer confirmed no separate clean Windows PC or VM is available.
Version-matched installer, portable, upgrade, uninstall/archive preservation
and signed updater acceptance cannot be completed in this session. A public
release or green CI does not close that acceptance gate.

## Local verification

- 480 Rust tests passed; one ignored. Formatting and strict workspace Clippy passed.
- All 16 catalogued fixtures and the validator rejection regressions passed.
- All 156 frontend tests passed with the normal command, and production build passed.
- The explicit CLI import command passed its smoke check.
- The final corpus sweep read 237 sessions / 104,495 events, with no unreadable
  files and 36 unrecognised events: 34 Claude away_summary records, one
  bridge_status record and one malformed line. These remain diagnostics, with
  doctor exit 1. The actively written corpus can grow after this receipt.

These local checks are separate from the Woodpecker results above. Neither
establishes clean-host acceptance or real capture certification of rare shapes.
