# ContextTrace — DevTools for AI Coding-Agent Context

> Historical design plan, written before implementation. It explains the
> architecture and original Milestone 1 scope, but its crate names, counts and
> work order are not the current plan. Use [../BACKLOG.md](../BACKLOG.md) for
> scheduled work and [MVP-STATUS.md](MVP-STATUS.md) for the current release
> assessment.

## Context

`C:\Users\anesk\source\repos\ContextTrace` is an empty git repo (initialized, zero commits). We are building a
local-first tool that answers, for any Codex CLI or Claude Code session:

> **What was in the model's context at this turn, where did it come from, how large was it, and how did it evolve?**

Not a chat-history viewer. The framing is **DevTools for agent context** — the workflow is
"why did the agent do that? → inspect turn → trace the source → find the 38k-token garbage tool result".

Before planning, I probed the real on-disk formats on this machine. The findings below **materially change**
the brief's assumptions and are the foundation of this plan.

---

## What the format investigation established

Corpus on this machine: **709 Claude Code sessions (336 MB)**, **61 Codex sessions (191 MB)**.
Largest single file **55 MB**. Largest observed single turn: **356,985 prompt tokens**.

### Codex CLI — `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`

Line envelope: `{timestamp, type, payload}`.

| `type` | Meaning for us |
|---|---|
| `session_meta` | `session_id, cwd, cli_version, model_provider, git{commit_hash,branch,repository_url}`, **`base_instructions`** (the literal system prompt text), `context_window` |
| `turn_context` | Per-turn `model, cwd, workspace_roots, approval_policy, sandbox_policy, personality` |
| `response_item` | **The actual OpenAI Responses API items.** Subtypes: `message`(user/assistant/developer), `reasoning`, `function_call`, `function_call_output`, `custom_tool_call(_output)`, `tool_search_call(_output)` |
| `event_msg/token_count` | `info.last_token_usage.input_tokens` = exact prompt size for that request; also `total_token_usage`, `model_context_window` |
| `compacted` | Carries **`replacement_history`** — the literal post-compaction item list |
| `world_state`, `event_msg/{task_started,task_complete,turn_aborted,sub_agent_activity,…}` | Session lifecycle |

**Consequence:** Codex context reconstruction is *replay*, not inference. Folding `response_item` lines in order
reproduces the request body. And because `compacted` stores the replacement list, **discarded content is exactly
derivable by diffing** — the brief's caution ("do not claim exact discarded content") is over-conservative for Codex.

### Claude Code — `~/.claude/projects/<slugified-cwd>/<session-uuid>.jsonl`

| `type` | Meaning for us |
|---|---|
| `assistant` | `message.usage{input_tokens, cache_creation_input_tokens, cache_read_input_tokens, output_tokens}` — **the three input figures summed are the exact prompt size**. Plus `model`, `requestId`, `stop_reason` |
| `user` | Prompts and `tool_result` blocks; `toolUseResult` holds structured result data |
| `attachment` | **The injected-context surface.** 21 subtypes observed: `file`, `nested_memory` (CLAUDE.md, with resolved path), `skill_listing`, `invoked_skills`, `dynamic_skill`, `agent_listing_delta`, `mcp_instructions_delta`, `deferred_tools_delta`, `hook_additional_context`, `hook_success`, `plan_file_reference`, `plan_mode(_exit/_reentry)`, `compact_file_reference`, `edited_text_file`, `task_reminder`, `queued_command`, `command_permissions`, `date_change`, `auto_mode` |
| `system` | `subtype`: **`compact_boundary`** (with `compactMetadata{trigger, preTokens, postTokens, cumulativeDroppedTokens, durationMs}` + `logicalParentUuid` bridging the boundary), `turn_duration`, `away_summary`, `local_command`, `model_refusal_fallback`, `scheduled_task_fire`, `bridge_status`, `informational` |
| `file-history-snapshot`, `pr-link`, `ai-title`, `mode`, … | Sidecar metadata |

**Two consequences that drive the design:**

1. **Events form a `parentUuid` DAG, not a flat list.** Rewinds/edits create sibling branches. Reading the file
   in line order and calling it "the conversation" is *wrong*. The context at an assistant turn is the
   **parent-chain walk** from that message back to root, with `logicalParentUuid` used to cross compact boundaries.
2. **Instruction provenance is observed data.** The `attachment` subtypes above already say *exactly* where each
   injected block came from. The brief scheduled "instruction tracing" for P2; it is largely free at P0 and moves up.

### The one genuine asymmetry

Codex is GPT-family → `tiktoken-rs` with `o200k_base` gives near-exact per-item counts.
Anthropic ships **no local tokenizer** → for Claude Code, per-item counts are necessarily estimates while the
per-turn *total* is exact. This asymmetry is precisely why "observed vs reconstructed" must be encoded in the
type system rather than in documentation.

---

## Architecture

```
~/.codex/sessions/*.jsonl        ~/.claude/projects/**/*.jsonl   (READ-ONLY, opened read-only)
              ↓                              ↓
        ct-adapters   (per-agent parsers, streaming, offset-recording)
              ↓
         ct-model     (normalized types; zero logic)
              ↓
        ct-context    (reconstruction engine + token accounting/calibration)
              ↓
       ct-analysis    (diagnostics: spikes, duplicates, waste)
              ↓
     ct-cli    /    ct-ui  (Tauri v2, later)
```

Cargo workspace. Each arrow is a crate dependency edge, so the brief's hard requirement — parsing separate from
reconstruction separate from analysis separate from presentation — is enforced **by the compiler**, not by convention.
Because Tauri's backend *is* Rust, `ct-ui` calls the same crates through `#[tauri::command]` with no sidecar,
no IPC protocol, and no logic duplication.

### Crates

| Crate | Responsibility | Key deps |
|---|---|---|
| `ct-model` | Normalized types only. No I/O, no logic. | `serde`, `chrono` |
| `ct-adapters` | `AgentAdapter` trait; `codex.rs`, `claude_code.rs`; discovery; streaming parse | `serde_json`, `walkdir` |
| `ct-context` | `ContextSnapshot` reconstruction, token counting, calibration | `tiktoken-rs` |
| `ct-analysis` | Largest contributors, diffs, lifecycles, diagnostics | — |
| `ct-index` | SQLite cache of derived metadata; disposable, rebuildable | `rusqlite` (bundled) |
| `ct-cli` | `clap` binary → single `.exe` | `clap`, `comfy-table` |
| `ct-ui` | Tauri v2 + React shell (**milestone 3**, not now) | — |

**Original privacy target:** no HTTP client anywhere in the dependency tree.
The current Tauri desktop stack makes that dependency-wide rule impractical:
general-purpose framework dependencies may contain network support even though
ContextTrace does not invoke it. The current enforceable boundary is no
application upload/telemetry code, a core-only Tauri capability, a local-IPC
CSP, read-only agent adapters, and release review of those permissions.

---

## Normalized data model (`ct-model`)

Design notes that matter more than the field lists:

**Honesty encoded in types.** Rather than a `confidence: String` field everyone forgets to set:

```rust
enum Confidence { Observed, Derived, Estimated }

enum TokenCount {
    Observed(u32),                          // agent reported it
    Exact(u32),                             // we tokenized it (tiktoken, Codex only)
    Calibrated { estimate: u32, scaled: u32 }, // heuristic, rescaled to a known total
    Estimated(u32),                         // heuristic, unreconciled
}
```

A `Calibrated` value cannot be printed as if exact, because the presentation layer must match on the variant.

**Never crash on schema drift.** Every adapter enum carries an unknown arm, and every struct keeps a
`#[serde(flatten)] extra: serde_json::Map` so new agent fields survive parsing and stay visible in the raw inspector:

```rust
enum EventKind {
    UserMessage(..), AssistantMessage(..), ToolCall(..), ToolResult(..),
    ContextInjection(..), Compaction(..), SessionMeta(..),
    Unknown { agent_type: String },   // <- new agent event types land here, parse succeeds
}
```

**Lazy content, always.** Events never own their raw text:

```rust
struct SourceRef { file_id: FileId, byte_offset: u64, byte_len: u32, line_no: u32 }
```

Content is seeked and read on demand. This is what makes 55 MB files with multi-megabyte lines (inline base64
images in Codex `compacted` payloads) tractable — the index holds offsets, never payloads.

**A `Turn` is one model request**, not one user exchange — because "what was in context at this turn" only has
meaning per API call. Claude Code: one `assistant` event bearing `usage`. Codex: `task_started`→`task_complete`
bounded by `turn_context`, sized by `token_count.last_token_usage`.

Other types per the brief: `AgentSession`, `SessionMetadata`, `Message`, `ToolCall`, `ToolResult`, `ContextItem`,
`TokenUsage`, `CompactionEvent`, `ContextSnapshot`. All optional fields genuinely optional — missing data is normal.

---

## Reconstruction engine (`ct-context`) — the heart

`ContextSnapshot::at(session, turn)` returns the best reconstruction of what the model saw, per-agent:

**Codex — replay.** Fold `response_item` entries in file order into a live item list; a `compacted` event replaces
that list wholesale with `replacement_history`. Membership confidence is `Observed`. Item token counts are `Exact`
via `tiktoken-rs`. `base_instructions` from `session_meta` is an observed context item.

**Claude Code — parent-chain walk.** From the target `assistant` event, walk `parentUuid` to root, crossing compact
boundaries via `logicalParentUuid`. This correctly excludes abandoned branches from rewinds. `attachment` events on
the chain become `ContextItem`s whose `category` and `source` are **observed** (e.g. `nested_memory` → category
`RepositoryInstructions`, source = the resolved CLAUDE.md path).

**Then calibrate.** Sum per-item estimates, compare to the exact observed total, scale to fit, and expose the
remainder as an explicit residual bucket:

```
Context — 146,820 tokens  [observed, exact]

  Tool outputs      61,240  41.7%  [calibrated]
  Conversation      37,820  25.8%  [calibrated]
  Instructions      19,110  13.0%  [calibrated]
  Repo context      14,420   9.8%  [calibrated]
  Unattributed      14,230   9.7%  [residual — system prompt + tool schemas]
```

Percentages become trustworthy even though individual counts stay approximate, and the thing we genuinely cannot
see (the hidden system prompt and tool JSON schemas) is named rather than silently smeared across the categories.

---

## Milestone 1 — CLI proves the core (the only milestone in scope now)

Per the brief: **do not touch UI until parse → normalize → reconstruct is reliable.**

Build order:

1. **Workspace scaffold** — `cargo new` workspace, 6 crates, CI running `cargo test` + `clippy -D warnings`.
2. **`ct-model`** — normalized types with the honesty/unknown/lazy patterns above.
3. **`AgentAdapter` trait** — `discover_sessions()`, `parse_session()`, `agent_kind()`.
4. **Discovery** — scan `~/.codex/sessions/**` and `~/.claude/projects/**`; respect `CODEX_HOME`/`CLAUDE_CONFIG_DIR` overrides; print every path read (brief: "clearly show which local paths are being read").
5. **Codex adapter** — streaming line parse recording `SourceRef` offsets.
6. **Claude Code adapter** — same, plus `parentUuid` DAG construction.
7. **Fixture tests** — hand-authored synthetic JSONL per event shape found in the probe: `compact_boundary`, each attachment subtype, Codex `replacement_history`, `token_count`, and a deliberate **unknown-event-type** case asserting graceful degradation.
8. **`ct sessions`** — table + `--json`; filters `--agent`, `--project`, `--since`.
9. **`ct inspect <id>`** — chronological timeline, collapsed tool output previews.
10. **Token accounting** — `tiktoken-rs` for Codex; heuristic + calibration for Claude Code.
11. **`ContextSnapshot::at()`** — both reconstruction strategies.
12. **`ct context <id> --turn N`** — the composition breakdown above.
13. **`ct largest <id> --turn N`** — top context consumers.
14. **`ct doctor <id>`** — first diagnostics: huge tool output, context spikes, compaction summary.

`--json` on every command from the start, so ContextTrace is pipeable into other tooling before the UI exists.

### Deferred (explicitly out of scope for milestone 1)

`ct diff A..B`, context-growth timeline, context lifecycle, search, duplicate/waste heuristics, the SQLite index
(milestone 1 parses on demand; add `ct-index` when a command feels slow, not before), and all Tauri work.
Adding the index prematurely means tuning a cache before knowing the access patterns.

---

## Verification

1. **`cargo test`** — fixture tests per adapter, including the unknown-event-type degradation case.
2. **Corpus smoke test** (the real acceptance gate, and it is already available locally): parse all **770** real
   sessions, assert **zero panics**, and emit a histogram of unrecognized event types. Any new agent event shows up
   here as a count, not a crash. Run as a gitignored local-only test; the corpus is never committed.
3. **Cross-check the headline number**: run `ct context <id> --turn N`, then read the raw JSONL for that turn and
   confirm the reported total equals `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`
   (Claude Code) or `info.last_token_usage.input_tokens` (Codex). If the reported total is not exactly the observed
   one, the calibration layer has a bug.
4. **Read-only proof**: record mtimes of `~/.codex` and `~/.claude` before and after a full run; assert unchanged.
5. **Dependency audit**: `cargo deny` confirms no network-capable crate is in the tree.

**Milestone 1 is done when** `ct sessions` → pick a session → `ct inspect <id>` → `ct context <id> --turn N` gives a
useful, correctly-totalled breakdown for both a Codex and a Claude Code session, with every number carrying its
provenance.
