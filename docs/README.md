# Documentation

This directory contains the public user, contributor and maintainer guides.
The repository root is the user-facing entry point; use this index to find the
supporting detail.

## For users

- [User guide](guide.md): commands and worked examples.
- [Input formats](formats.md): persisted Codex CLI and Claude Code JSONL.
- [Format compatibility](FORMAT-COMPATIBILITY.md): versioned evidence,
  fixtures and the update process.
- [Methodology](methodology.md): what the measurements mean and their limits.
- [Cache and context usage](cache-and-context-usage.md): cache token shares, pricing coverage and used/total context.
- [Notifications](notifications.md): desktop notification rules and settings.
- [Local files](local-files.md): opening recorded file targets from context composition.
- [Projects and resume](projects-and-resume.md): temporary workspace grouping, project visibility and native session continuation.
- [Updater](UPDATER.md): update behavior and release feed.

## Project and maintenance

- [Architecture](architecture.md): crate boundaries, invariants and tests.
- [CI](CI.md): automated checks, local equivalents and coverage limits.
- [Release procedure](RELEASING.md): Windows packaging and acceptance gates.
- [MVP status](MVP-STATUS.md): current implementation and publication state.
- [Maintainer handoff](../HANDOFF.md): completed work, validation and ordered next steps.
- [2026-10-07 format audit](FORMAT-AUDIT-2026-10-07.md): parser findings and coverage gaps.
- [Roadmap](ROADMAP.md): public, non-committal areas of future work.

The compatibility manifest and synthetic/redacted fixtures are maintained in
the repository alongside the parser tests. Private session corpora, signing
material, machine-specific runner configuration and unpublished credentials
are not part of the public documentation.

- [Parser audit follow-up](FORMAT-FOLLOWUP-2026-10-08.md): capture receipts, replay semantics, bounded input and acceptance limits.
- [Saved stream imports](STREAM-IMPORTS.md): explicit stdout/app-server timeline import contracts.
