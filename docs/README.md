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
- [Notifications](notifications.md): desktop notification rules and settings.
- [Updater](UPDATER.md): update behavior and release feed.

## Project and maintenance

- [Architecture](architecture.md): crate boundaries, invariants and tests.
- [CI](CI.md): automated checks, local equivalents and coverage limits.
- [Release procedure](RELEASING.md): Windows packaging and acceptance gates.
- [MVP status](MVP-STATUS.md): current implementation and publication state.
- [Roadmap](ROADMAP.md): public, non-committal areas of future work.

The compatibility manifest and synthetic/redacted fixtures are maintained in
the repository alongside the parser tests. Private session corpora, signing
material, machine-specific runner configuration and unpublished credentials
are not part of the public documentation.
