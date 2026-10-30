# Documentation

The [repository README](../README.md) is the main entry point. These guides
provide additional detail for users and contributors.

## User guides

- [User guide](guide.md): commands and worked examples.
- [Input formats](formats.md) and [format compatibility](FORMAT-COMPATIBILITY.md): persisted session formats, fixtures and evidence limits.
- [Saved stream imports](STREAM-IMPORTS.md): stdout and app-server timeline imports.
- [Methodology](methodology.md): measurements and their limits.
- [Cache and context usage](cache-and-context-usage.md): token shares, pricing and context windows.
- [Notifications](notifications.md): desktop rules and settings.
- [Local files](local-files.md): opening recorded file targets.
- [Projects and resume](projects-and-resume.md): project visibility and native session continuation.
- [Instruction-file comparison](instruction-file-comparison.md): local reader limits and refusal states.
- [Archive storage and recovery](archive-storage.md): archive layout and interrupted-write recovery.
- [Updater](UPDATER.md): update behavior and release feed.
- [Linux](LINUX.md) and [macOS](MACOS.md): platform build instructions and validation limits.

## Project and maintenance

- [Architecture](architecture.md): crate boundaries, invariants and tests.
- [CI](CI.md): automated checks, local equivalents and coverage limits.
- [Release procedure](RELEASING.md): packaging and acceptance gates.
- [Release notes](releases/v0.1.18.md): changes and retained limits.
- [Roadmap](ROADMAP.md): areas of future work, without commitments.

The compatibility manifest and synthetic or redacted fixtures are maintained
alongside the parser tests. Private session corpora, signing material,
machine-specific runner configuration and unpublished credentials are not
part of the public documentation.
