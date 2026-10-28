# Roadmap

ContextTrace is developed around a narrow goal: help a user understand what a
Codex CLI or Claude Code session recorded, how context changed, and which
measurements are observed versus estimated. This page describes areas of
interest, not a schedule or commitment.

## Current focus

The [maintainer handoff](../HANDOFF.md) and [engineering backlog](BACKLOG.md)
record the current FEEDBACK audit follow-ups: 12 findings have remediation
checkpoints and 22 remain open. Export writer coordination before bounded
native worker dispatch, notification fidelity/cursors and Claude calibration
scaling are next. Earlier parser/capture acceptance gaps remain explicit.

- Keep Windows releases validated and published through Woodpecker; verify
  updater behavior and installation separately on a clean host.
- Validate native session restoration and visible terminal behavior with real
  installed Codex/Claude CLIs; automated stub tests cover command handling.
- Keep persisted-session format support evidence-based: track producer
  versions, add redacted fixtures for new shapes, and test semantic behavior.
- Improve diagnostics and communicate uncertainty without implying that logs
  reveal context the agent never persisted.

## Possible future work

- Additional agent adapters, if their local persisted formats can be tested.
- More ways to compare sessions and explore context changes over time.
- Performance improvements driven by reproducible measurements.

There is no promise that every item will be implemented. User-visible scope and
release gates are recorded in [MVP status](MVP-STATUS.md).
