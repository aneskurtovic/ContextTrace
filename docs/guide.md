# User guide

This guide describes common workflows. All IDs, paths and sample values below
are fictional; examples do not reproduce local session data. For the meaning
and limits of measurements, see [methodology](methodology.md); for input
structure, see [formats](formats.md). Back to the [README](../README.md).

## Getting the numbers out

Export a session as typed NDJSON. The command writes to standard output, so you
choose whether and where to save it:

```powershell
ct export <session-id> --redact-secrets > session.ndjson
```

The header records the schema, estimator and redaction mode. Turn and item
records include confidence and provenance. Keep in mind that a session export
can contain prompts, source code and terminal output; inspect it before sharing.

## Finding credentials without disclosing them again

`ct secrets <session-id>` reports credential-shaped matches by type and
location, not by value. It does not prove that a match is a live credential,
and it cannot recognize every provider-specific format. Review the original
session locally; do not paste raw logs into an issue.

The desktop Context Doctor follows the same value-free principle. Redacted
export is available in both interfaces; archive redaction is enabled by
default unless `--raw` is explicitly requested.

## Catching an agent that changed its format

Run a format sweep against a selected local root:

```powershell
ct doctor --dir codex
ct doctor --dir claude-code
```

The command reports event shapes it does not recognize. Its result describes
the selected files and this build only; it does not promise compatibility with
every producer version. When a new shape appears, contribute a redacted
fixture and semantic regression test. The
[compatibility policy](FORMAT-COMPATIBILITY.md) describes the process.

## Filtering without lying about the whole

Filter the largest contributors to a turn by source, category, confidence or
minimum size:

```powershell
ct largest <session-id> --category tool-outputs --min-tokens 2000
ct context <session-id> --turn 12 --source tool:Bash
```

Percentages remain relative to the full observed prompt total, not the filtered
subset. The unattributed remainder has no provenance and therefore does not
match a source filter; request its category explicitly if needed.

## Exact Codex compaction diffs, without printing prompt content

For Codex sessions whose persisted records include a usable
`replacement_history`, `ct compactions <session-id>` compares the items before
and after a compaction. It reports structural metadata, not prompt or tool
output text. Missing, malformed or oversized records may make a boundary
unavailable. Claude Code does not persist the same literal replacement list,
so ContextTrace does not claim an exact Claude compaction diff.

## One item's lifecycle, and the difference between gone and evicted

Use an item identifier from `ct largest` or a distinctive label:

```powershell
ct trace <session-id> --item <item-id>
```

The trace reports observed presence across reconstructed turns. A missing
record or failed reconstruction is not evidence of eviction. On Claude Code,
a branch change can explain disappearance without a compaction; for Codex, a
departure without a recorded compaction is reported conservatively.

## Comparing two turns without comparing two rulers

Compare turns in one session or compare each session's largest measured turn:

```powershell
ct diff <session-id>@12 <session-id>@18
ct diff <left-session>..<right-session>
```

Claude Code item counts are estimates calibrated to reported prompt totals.
When comparing sessions, the command carries a bound for differences the
calibration instruments alone could explain. Codex/Claude token deltas are
withheld when the underlying measurements are not comparable.

## The whole session at once, using only what the agent reported

```powershell
ct growth <session-id>
```

The chart uses the agent's recorded prompt totals. It marks compactions and
missing usage as gaps rather than inventing values. Each column summarizes a
range of turns, so a change within a column may not be visible at that chart
resolution.
