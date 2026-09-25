# Methodology

ContextTrace separates what an agent explicitly recorded from reconstructed
or estimated values. This page explains the measurements without using private
session corpus statistics. All examples are schematic.

Back to the [README](../README.md) · see also [formats](formats.md) and
[architecture](architecture.md).

## Observed totals and estimated items

Persisted usage records can provide an observed prompt total for a turn. They
do not necessarily expose the complete prompt as separate text items. Codex
items are estimated by default; `ct context --exact` and `ct largest --exact`
can tokenize eligible plain-text Codex items. Structured, encrypted, oversized
and image-bearing content cannot always be represented by plain-text token
counts and remains estimated or unavailable. Claude Code item sizes are
estimates because a matching local tokenizer is not available; reported turn
totals can still be observed.

The command output identifies confidence. Exact mode is not supported for
Claude Code and refuses rather than relabeling an estimate as exact.

## Per-session calibration

For Claude Code, ContextTrace fits a characters-per-token ratio using changes
between consecutive turns with usable reported totals. It then scales item
estimates to the observed total. The residual is the difference not attributed
to reconstructed items; it may include system instructions, tool schemas,
formatting overhead and estimation error. It is not a literal reconstruction
of hidden prompt text.

Calibration is session-specific because prose, source code, paths and structured
data tokenize differently. A ratio or scale factor is a property of the
available measurements, not a universal constant. The output reports the fit
and its spread so a reader can judge its stability.

## Incomplete or conflicting evidence

An event can be persisted without enough information to assign its size or
turn. Some usage records combine multiple API calls; reasoning text may be
redacted; some payloads are opaque or oversized. ContextTrace marks these cases
instead of manufacturing precision.

Reconstruction can also account for more content than an agent's reported
prompt total. The cause of such a discrepancy may not be recorded. In that
case, the residual is not measurable separately from the over-count, and the
tool says so rather than clamping it to zero or inventing a removal event.

## Comparisons

Percentages are shares of the observed total when one exists. Filtering does
not change the denominator to the matching subset. Comparing Claude Code
sessions carries calibration uncertainty; Codex and Claude Code token deltas
are not presented as directly comparable when their measurement instruments
differ.

`ct growth` uses reported prompt totals only. Missing totals are shown as gaps,
not as zero-sized prompts. `ct trace` answers whether an item is present in a
reconstructed turn; absence from an unreconstructable turn is unknown, not an
eviction.

## Safety and reproducibility

Session logs may contain prompts, code and credentials. ContextTrace reads
agent roots without modifying them; examples and committed fixtures are
synthetic or redacted. A local corpus sweep is useful for detecting drift but
is machine-specific evidence. The public compatibility claim is bounded by
the committed producer-version fixtures and tests.
