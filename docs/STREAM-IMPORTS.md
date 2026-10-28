# Saved stdout and app-server captures

Use the explicit CLI importer for saved presentation streams:

```powershell
ct import capture.jsonl --surface codex-exec --producer-version 0.161.0
ct import notifications.jsonl --surface codex-app-server --producer-version 0.161.0
ct import capture.jsonl --surface claude-stream --producer-version 2.1.293
```

The command prints a JSON report and leaves the input untouched. It returns 1
when unsupported or unreadable records need review, and rejects a mismatched
surface, mixed session identities, or a Claude init version that disagrees with
the supplied version. Record `codex --version` or `claude --version` when
capturing a stream; Codex stdout does not include its binary version.

These are separate adapters from persisted sessions. `StreamAdapter` discovery
reads only an explicitly supplied capture directory. The normal desktop and
`ct sessions` discovery roots remain persisted-session roots; imports are not
silently installed into those roots or added to the desktop catalog.

Finalized Codex messages, reasoning and command output are normalized into
timeline events. Started/updated items and deltas stay presentation metadata;
repeated completed item IDs do not duplicate content. Claude messages preserve
individual text, tool, thinking and media blocks; subagent messages stay outside
the parent's content and usage. Unknown records and nested block types remain
visible diagnostics. Other tool/item variants without a tested semantic
contract are reported as unrecognised rather than guessed.

Usage is retained with its original scope and source byte range:

| Surface | Usage scope |
|---|---|
| Codex `exec --json` | Turn aggregate |
| Codex app-server | Last-request snapshot from `thread/tokenUsage/updated` |
| Claude `stream-json` assistant | Request usage, once per request identity |
| Claude result | Session aggregate |

The importer never adds these scopes together or labels an aggregate as one
request's prompt size. A repeated app-server last-request snapshot remains a
snapshot, not another billed request. Incomplete captures can still expose a
timeline; missing completion records or usage are not inferred.

Presentation streams omit request history, instructions, tool definitions and
other model-visible context. Context reconstruction, compaction membership
diffs, and exact recount are therefore unavailable. The report has no invented
context turns and states these limits explicitly.

The fixture catalog records reviewed local Codex 0.161.0 and Claude 2.1.293
stdout captures separately from persisted captures. The app-server fixture is
synthetic, pinned to upstream commit
`979011409de0a60b52f179721948e65531d26144`; it is not a real capture or an
exhaustive certification of app-server notifications.
