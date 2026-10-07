# Maintainer handoff

Updated: **2026-10-07**. Start with this file when continuing the parser audit.

## Completed

The persisted Codex/Claude JSONL audit and parser fixes are documented in
[the audit report](docs/FORMAT-AUDIT-2026-10-07.md). Changes cover structured
tool-search arguments, legacy shell calls, additional tool definitions,
compaction aliases, opaque content, retained review evidence and Claude
unreadable-record diagnostics. The fixture catalog now has 11 entries,
including a synthetic contract pinned to the Codex 0.161.0 upstream schema.

Local verification passed: 468 Rust tests (one ignored), formatting, workspace
Clippy with warnings denied, fixture validation and validator regressions,
156 frontend tests, and the frontend production build. Frontend tests needed
one thread and a 15-second timeout after Windows worker/timing failures; no
frontend source was changed. Dependencies were already installed.

The final local corpus sweep read 229 sessions / 100,651 events, with no
unreadable files. The only remaining unrecognised event was one malformed
Claude line. `doctor` correctly exited 1. Do not remove or silently accept
that warning merely to make the sweep green. Private corpora stay local.

## Next steps, in order

1. Check the Woodpecker `frontend`, `rust` and `windows` push statuses on the
   exact commit containing this handoff. Local results above do not establish
   CI success. Use Woodpecker only; do not dispatch GitHub Actions.
2. Capture and review a minimal redacted persisted Claude **2.1.293** transcript.
   The installed binary was 2.1.293, but the recent-file sample only observed
   records up to 2.1.292. Add its catalog entry and semantic assertions before
   claiming version-matched evidence. Verify newer producer releases again
   when resuming; the version snapshot is dated.
3. Add reviewed Codex **0.161.0** capture evidence for the newly covered shapes.
   A local sample contained that producer version, but the new upstream fixture
   is synthetic and cannot certify every feature of the release.
4. Investigate Codex `image_generation_call` and `context_compaction` replay
   semantics with pinned upstream definitions and fixtures. The request-only
   `compaction_trigger` is not a durable response item. Do not guess at their
   context effects or classify them as harmless solely to eliminate warnings.
5. Improve nested drift detection: unknown Codex `event_msg` and Claude
   `system` subtypes currently become generic session events. Add tests that
   expose novel context-bearing shapes while preserving known metadata.
6. Review media accounting and oversized-input limits. Audio, file-based images
   and encrypted content use opaque proxies; reference length is not media
   token cost. The shared reader still buffers an entire raw line even when
   it declines to materialize the JSON tree; type sniffing is heuristic.
7. Keep stdout support a separately scoped adapter task: Codex `exec --json`,
   app-server and Claude `stream-json` are not persisted-session schemas.
   Add separate discovery/import contracts and fixtures if that scope is chosen.

## Verification commands

Follow [CLAUDE.md](CLAUDE.md) and [CI](docs/CI.md). Core audit commands:

```powershell
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/check-fixture-manifest.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/test-fixture-manifest.ps1
npm test --prefix crates/ct-ui
npm run build --prefix crates/ct-ui
cargo run -p ct-cli --locked -- doctor --dir --json
```

For the observed Windows test-worker issue, the successful diagnostic retry was:

```powershell
npm test --prefix crates/ct-ui -- --pool=threads --maxWorkers=1 --testTimeout=15000 --reporter=dot
```

The default CI command remains unchanged. Corpus counts change as agents append
records; inspect diagnostics rather than expecting an identical event count.

## Release state

The public latest stable release was verified as **v0.1.8**, published
2026-10-01, with all six expected asset names present. Its source commit
`3afd229` has successful Woodpecker push checks in pipeline 86 and tag release
checks in pipeline 87. This audit does not reverify downloaded asset bytes,
installer acceptance or updater acceptance. No new release/tag is requested.

For a later release, follow [RELEASING.md](docs/RELEASING.md): exact-commit
Woodpecker validation, Windows tag packaging/publishing/asset verification,
then separate clean-host installer, portable, upgrade, uninstall/archive
preservation and signed updater acceptance. Historical 0.1.3 acceptance is not
evidence for the current release. See [MVP status](docs/MVP-STATUS.md).
