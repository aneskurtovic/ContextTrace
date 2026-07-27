//! Anti-corruption layer for Claude Code.
//!
//! **Status: not yet implemented.** This module is a placeholder so the
//! workspace builds; it does not implement
//! [`AgentAdapter`](ct_domain::ports::AgentAdapter) yet. The notes below record
//! what the format investigation established, so the implementation does not
//! have to rediscover it.
//!
//! # The foreign model
//!
//! Claude Code writes `~/.claude/projects/<slugified-cwd>/<session-uuid>.jsonl`.
//! Three properties drive the whole design, and each contradicts a
//! reasonable-sounding assumption:
//!
//! ## 1. The file is a DAG, not a transcript
//!
//! Every line carries `uuid` and `parentUuid`. Rewinds and edits create sibling
//! branches *in the same file*, so line order is not conversation order.
//! Reading top to bottom and calling it "the context" silently includes
//! abandoned branches the model was never shown. The context at a turn is the
//! parent chain walked back from that turn's assistant message.
//!
//! `compact_boundary` events carry `logicalParentUuid`, pointing at the
//! pre-compaction event they continue from. That link must **not** be followed
//! when reconstructing membership -- the history it points at was summarised
//! away and was not in the prompt. It is for lifecycle and diff views, which
//! answer "what was dropped", not "what was present".
//!
//! ## 2. One API response spans several lines
//!
//! A single model request is written as multiple `assistant` lines -- one for
//! thinking, one for text, one per tool call -- all sharing a `requestId` and
//! all carrying an *identical* `usage` object. Treating each line as a turn
//! would inflate the turn count and report the same prompt size repeatedly, so
//! lines must be grouped by `requestId`.
//!
//! ## 3. Injected context is labelled at the source
//!
//! `attachment` lines record *why* something entered the prompt. The subtypes
//! observed in a 709-session corpus, and their domain mapping:
//!
//! | attachment type | category | source |
//! |---|---|---|
//! | `nested_memory` | repository instructions | the CLAUDE.md path |
//! | `file`, `edited_text_file` | file contents | the file path |
//! | `skill_listing`, `invoked_skills`, `dynamic_skill` | developer instructions | harness |
//! | `agent_listing_delta`, `mcp_instructions_delta`, `deferred_tools_delta` | tool definitions | harness |
//! | `hook_additional_context`, `hook_success` | developer instructions | the hook |
//! | `plan_file_reference`, `plan_mode*` | developer instructions | harness |
//! | `compact_file_reference` | summaries | compaction |
//! | `task_reminder`, `queued_command`, `command_permissions`, `date_change`, `auto_mode` | other | harness |
//!
//! This is instruction provenance as *observed data*, which is why it can ship
//! at P0 rather than being a research project.
//!
//! # What we cannot do
//!
//! Anthropic ships no local tokenizer, so per-item counts are always estimates.
//! The per-turn *total* is exact -- `usage.input_tokens +
//! cache_creation_input_tokens + cache_read_input_tokens` -- and
//! [`TokenCalibrator`](ct_domain::services::TokenCalibrator) reconciles the two.

/// Reads Claude Code sessions. Not yet implemented.
pub struct ClaudeCodeAdapter;
