# IDEAS.md — Living Backlog (Updated)

> This is the unscheduled idea pool. It is intentionally broader than the MVP
> and may describe work that has since shipped, changed shape or been dropped.
> [BACKLOG.md](BACKLOG.md) is the authoritative work queue;
> [docs/MVP-STATUS.md](docs/MVP-STATUS.md) defines the current release gates.

> **This file is an idea pool, not a plan.** Nothing here is scheduled until it
> is pulled into [`BACKLOG.md`](BACKLOG.md) and given a `CT-nnn` id. Items that
> have been pulled in carry a `[→ CT-nnn]` marker — follow it to `BACKLOG.md`
> for status, rationale and the condition that closes it. Unmarked items are
> unscheduled: interesting, but not committed to.

This document serves as a "living backlog" of features and conceptual improvements for ContextTrace. These ideas are categorized by their functional area and prioritized by their impact on the "Why did the agent do that?" workflow.

---

## 1. Domain-Model Driven Features (The "Confidence & Calibration" Layer)

**Goal:** Leverage ContextTrace's strict Observed vs. Estimated type system to provide insights no other tool can.

- **Confidence Heatmaps:** In the UI, color-code every context item by its TokenCount variant. Bright green for Observed (e.g., Codex tiktoken counts), yellow for Calibrated (Claude Code scaled estimates), and red for Estimated. Make the uncertainty visually unavoidable.  
  → *Expanded:* Add a hover tooltip that shows the exact TokenCount variant + provenance chain (e.g., *"Calibrated from base estimate 12 400 → scaled by 0.91 against observed total"*). Also expose a CLI flag `--confidence-colors` that emits ANSI‑colored output for terminal users, and a `--confidence-emojis` flag for terminals without colour (✅ observed, ⚡ calibrated, ❓ estimated, ⚠️ residual).  
  → Integrate SARIF linting so any confidence‑laundering violation (Observed combined with Estimated presented as Observed) is emitted as a SARIF result that can be surfaced directly in the heatmap UI and in CI.  
  → **New:** *Confidence decay* – when an observed item is later transformed (e.g., compacted), its confidence should be marked as `observed_at_then` with a timestamp, not just `observed`. Show a full provenance chain visually.

- **Calibration Drill-Down:** A detailed view for Claude Code turns showing the math: *“Base estimate: 162,400 tokens. Known total: 146,820. Scale factor applied: 0.904.”* Allow users to see what the estimator thought before it was forced to match the API reality.  `[→ CT-014]`
  → *Expanded:* Surface the per-category scale factors when they diverge significantly (e.g., tool outputs scaled by 0.7 while conversation scaled by 0.95). This can reveal systematic bias in the tokenizer approximation and guide future estimator improvements.  
  → **New:** `ct calibration-plot <id> --format svg` – outputs a terminal‑friendly plot (or SVG) showing how scale factors drift across turns. Large swings indicate systematic estimation errors in certain session phases.  
  → **New:** Calibration confidence intervals – since many estimators are statistical, show a range (e.g., *"Base estimate: 162,400 ± 3,200 tokens"*) and derive a confidence interval for the final calibrated figure.  
  → **New:** Calibration explosion alert – if the scale factor exceeds 2.0 or falls below 0.5, flag the turn as *high‑risk*; this often points to a tokenizer bug, a new model version, or an unexpected encoding.

- **Residual Forensics:** Since Claude Code turns always feature an explicit "Unattributed" residual (system prompt + tool schemas), provide a `ct residual <id> --turn-range 1..50` command. If the residual spikes by 5k tokens unexpectedly, it means the agent dynamically registered a new tool or the system prompt mutated.  `[→ CT-016]`
  → *Expanded:* Automatically annotate residual spikes with the nearest preceding events that could explain them (new tool registration, `CLAUDE.md` change, model switch). Emit a structured `residual_delta` event that scripts can watch.  
  → **New:** *Residual decomposition* – break the residual into finer components: system prompt overhead, tool schema formatting (XML/JSON wrappers), protocol cruft, and agent‑specific template strings. This helps pinpoint the true source.  
  → **New:** *Residual blaming* – produce a ranked list of likely culprits for a residual spike, correlated with nearby events (tool registrations, `AGENTS.md` updates, file changes).  
  → **New:** *Residual regression tests* – store the baseline residual distribution (mean, median, standard deviation) for a given project/model, and alert when a new session deviates beyond a configurable threshold.

- **Compaction Diff Engine (Codex):** Because Codex compacted payloads contain `replacement_history`, ContextTrace can definitively show what was actually discarded. A view that explicitly lists: *“These 4 files and 2 tool outputs were removed from context to save 80k tokens.”*  `[→ CT-027]`
  → *Expanded:* Rank discarded items by *regret score* = tokens saved × how many subsequent turns referenced the same content (via fuzzy match or embedding). High‑regret discards become the most valuable debugging signals.  
  → **New:** *Compaction effectiveness* – show the ratio of tokens removed vs. tokens that were actually needed later (using future references). This can be used to suggest better compaction strategies.

- **Confidence Laundering Alerts:** A strict lint in the app layer: if a use case ever attempts to mathematically combine an Observed value with an Estimated value and present the result as Observed, the application panics or logs a critical domain violation.  `[→ CT-002]`
  → *Expanded:* Make the lint produce a machine‑readable SARIF report so it can be run in CI against any new use‑case code. Also expose a `ct domain-lint` command for library consumers. Surface the same SARIF results inside the Confidence Heatmaps UI.  
  → **New:** *Confidence laundering as a health metric* – track how often laundering would have occurred (if not caught) across all use‑cases; report it as part of `ct doctor` fidelity score.

---

## 2. Advanced CLI & Scripting (Immediate Next Steps)

**Goal:** Make the CLI a first‑class citizen for developers who want to pipe context data into `jq`, `duckdb`, or custom scripts.

- **Strictly Typed --json Output:** Every CLI command must output JSON schemas that are perfectly stable. Use sum types for states (e.g., `"status": "observed" | "calibrated" | "residual"`) so downstream scripts don't have to regex strings.  `[→ CT-008]`
  → *Expanded:* Ship JSON Schema files alongside the binary (`ct schema sessions`, `ct schema context`) and generate TypeScript / Python / Go bindings from them so typed clients stay in sync automatically.  
  → **New:** Support `--json-pretty` for human‑readable debugging.

- **Context Item Tracing (`ct trace`):** `ct trace <id> --item <hash-or-uuid>` follows a specific tool output or file snippet across its entire lifecycle. Turn 1: Injected (2,400 tokens). Turn 5: Still present. Turn 12: Evicted via compaction.  `[→ CT-018]`
  → *Expanded:* Support fuzzy matching (`--item-content "error: cannot find symbol"`) and content‑hash prefixes so users don't need the exact UUID. Add a `--lifecycle` mode that emits a compact timeline suitable for mermaid or PlantUML.  
  → **New:** `ct trace` with `--turn-range` to focus on a specific window.

- **Filtering by Provenance:** `ct context <id> --turn 42 --filter "source=tool_output" --filter "confidence=estimated"`. Essential for finding the *"38k‑token garbage tool result"* mentioned in the core workflow.  `[→ CT-017]`
  → *Expanded:* Allow compound filters with boolean logic and size predicates: `--filter "source=tool_output AND tokens>10000 AND confidence!=observed"`. Pipe‑friendly and powerful for hunting context bloat.  
  → **New:** Pre‑defined filter presets (e.g., `--filter-preset giant-tools`, `--filter-preset low-confidence`).

- **DuckDB Export:** `ct export <id> --format duckdb`. Outputs a local `.duckdb` file where turns, items, token counts, and provenance are tables. Allows analysts to write SQL: `SELECT sum(tokens) FROM context_items WHERE turn > 20 AND type = 'tool_output';`  `[→ CT-032]`
  → *Expanded:* Also support `--format parquet` and `--format ndjson` for lighter‑weight pipelines.  `[→ CT-020]` Pre‑create useful views (`v_context_bloat`, `v_compaction_events`, `v_residual_spikes`) so analysts don't have to reinvent the joins.  
  → **New:** `ct export` with `--include-metadata` to also write session metadata (model, start/end time, total cost) into the export.

- **Format Delta Reporting (`ct doctor`):** Extend the local corpus smoke test into a CLI command. `ct doctor --dir ./claude-sessions` outputs a histogram of unknown event types, warning the user: *"Claude Code updated yesterday. 3 unrecognized event types detected in your recent logs. Context reconstruction may be incomplete."*  `[→ CT-019]`
  → *Expanded:* Add `--baseline <path-to-previous-doctor-report>` so CI can fail a PR that introduces a regression in recognition rate. Also emit a machine‑readable fidelity score that can be tracked over time.  
  → **New:** `ct doctor` with `--watch` – continuously monitor a directory for new logs and report fidelity changes in real‑time.

---

## 3. Advanced Observability & The "Time-Machine"

**Goal:** Move from a static log viewer to a dynamic, temporal exploration of the model's mind.

- **Temporal Scrubbing (The Time-Travel Slider):** (Future Tauri UI) A playback bar at the bottom. Scrubbing updates the entire dashboard—Context Composition, Token Counts, and File Explorer—to show the exact state at that millisecond.  
  → *Expanded:* Keyboard‑driven scrubbing (`j`/`k`, space, `shift+arrows`) and a *"diff mode"* that highlights only what changed between the current and previous frame. Essential for long sessions.  
  → **New:** *Bookmarks* – mark interesting turns for quick navigation.

- **Token Treemap:** (Future Tauri UI) Replace force‑directed graphs with a strict treemap. Area strictly equals token count. Blocks are colored by source (Repo, Tool, Conversation). The eye naturally gravitates toward the giant red square representing a bloated tool output.  
  → *Expanded:* Support drill‑down (click a category → expand into individual items) and a *"stable layout"* option that keeps the same spatial arrangement across turns so the eye can track growth/shrinkage.  
  → **New:** *Aggregated treemap* – show the average context composition over a range of turns to spot structural patterns.

- **Ghost Context (Overlaying Turns):** Select two turns and see a *"Ghost"* view. Turn A’s context is shown in red, Turn B’s in green. Easily see what was *"forgotten"* or *"evicted"* by the agent's compaction logic.  
  → *Expanded:* Three‑way ghost (before compaction / after compaction / final) and a *persistence score* for each item (how many turns it survived).  
  → **New:** *Ghost diff* – show a unified diff of the actual text contents of the context, not just token counts, to see what textual information was lost.

- **Context Serialization Export:** Export a *"Context Bundle"* for a specific turn as a single Markdown file containing exactly what the model saw, formatted for human reading or pasting into a separate LLM chat to ask, *"Why did you do this?"*  
  → *Expanded:* Also produce a *"Context Bundle with Annotations"* that interleaves the original items with ContextTrace metadata (token counts, provenance, confidence). Ideal for post‑mortems and sharing with teammates who don't have ContextTrace installed.  
  → **New:** Support exporting as HTML with interactive tooltips for confidence and provenance.

---

## 4. The "Context Doctor" (Diagnostics & Read-Only Analysis)

**Goal:** Provide actionable feedback on why an agent session is failing or getting expensive, without ever modifying the source agent logs.

- **Waste Detection (Low-Entropy Context):** Identify large blocks of context containing very little information (e.g., massive `node_modules` paths, repeated build logs, minified vendor files) using simple heuristics like gzip compression ratios.  `[→ CT-024]`
  → *Expanded:* Rank waste by *"tokens × (1 − compression ratio)"* and surface the top offenders with a one‑line recommendation (*"Consider adding node_modules to the agent's ignore list"* or *"This build log was injected 7 times"*).  
  → **New:** *Waste over time* – show a plot of wasted tokens per turn to identify when bloat was introduced.

- **Instruction Drift Detection (Read-Only):** Instead of modifying `CLAUDE.md` (which violates the read‑only principle), ContextTrace diffs the `CLAUDE.md` currently on disk against the one embedded in the session log. Alert: *"The rules you have now are 400 tokens larger than what the agent saw during this session."*  
  → *Expanded:* Show a unified diff of the instruction files and highlight sections that were added/removed. Also detect when the agent was using a stale system prompt that no longer matches the repo's current `CLAUDE.md` / `AGENTS.md`.  
  → **New:** *Drift impact analysis* – correlate instruction drift with changes in agent behaviour (e.g., tool selection, verbosity) to quantify the effect.

- **Duplicate Context Alert:** Find instances where the exact same file content or tool output was injected into the context multiple times (a common bug in agent retry loops).  `[→ CT-023]`
  → *Expanded:* Detect near‑duplicates (normalised whitespace, stripped line numbers) as well as exact matches. Report the cumulative token cost of the duplicates.  
  → **New:** *Duplicate patterns* – categorise duplicates by source (file, tool, system) and suggest upstream fixes (e.g., caching tool outputs globally).

- **Privacy/Secret Leak Scanning:** Proactively scan the reconstructed context snapshots for regex matches of potential secrets (API keys, SSH keys, `.env` vars) that were accidentally sent to the cloud.  `[→ CT-025]`
  → *Expanded:* Ship a curated set of high‑precision patterns (OpenAI, Anthropic, AWS, GitHub, etc.) and allow users to add custom patterns via a local config file. Never upload the matches — only report *"secret of type X found at turn Y, item Z"*.  
  → **New:** *Secret leak timeline* – show when each secret was first introduced and how many turns it remained in context.

- **Cost Projection:** Calculate the *"Real‑World Cost"* of a session based on hardcoded provider pricing tables (OpenAI/Anthropic). Break it down by category: *"Tool outputs cost you $2.40; Conversation history cost $0.80."*  `[→ CT-026]`
  → *Expanded:* Support user‑supplied pricing overrides and multi‑model sessions. Also project *"what‑if"* costs: *"If tool outputs had been truncated at 4k tokens, this session would have cost $1.10 instead of $3.20."*  
  → **New:** *Cost forecasting* – given a partially completed session, estimate the total cost based on current trends.

---

## 5. Format Resilience & Anti-Corruption Layers (ACL)

**Goal:** Ensure ContextTrace degrades gracefully when agent formats change upstream.

- **Adapter Fidelity Score:** When parsing a session, report what percentage of the raw JSONL bytes were successfully mapped to domain entities vs. skipped as unknowns. A score of 100% means perfect reconstruction; 85% means something new was added to the agent.  `[→ CT-010]`
  → *Expanded:* Break the score down by event type and emit a *"fidelity report"* that can be tracked in CI. A sudden drop should open a GitHub issue automatically (optional, local webhook).  
  → **New:** *Fidelity over time* – store fidelity scores per session and show a trend graph to detect gradual format rot.

- **Raw vs. Reconciled View:** A toggle in the UI/CLI to switch between the *"Domain View"* (clean `ContextSnapshot` aggregate) and the *"Raw ACL View"* (the literal JSONL lines that contributed to it). Essential for debugging the tool itself.  
  → *Expanded:* In CLI form, `ct inspect <id> --raw` should pretty‑print the original JSONL lines that were used to build each domain entity, with line numbers from the source file.  
  → **New:** *Raw search* – allow searching the raw JSONL for arbitrary strings, with the results mapped back to domain entities.

- **Rust-Only Plugin Boundary:** Instead of a JS/Python plugin system (which breaks the minimal‑dependency/auditability invariant), define a `ct-adapters-macros` crate. Third parties can implement the `AgentSessionPort` trait in a separate Rust crate, compile it to a `.dll`/`.so`, and load it via a strict ABI boundary.  
  → *Expanded:* Provide a well‑documented example adapter crate (e.g., for Cursor or Gemini CLI) and a `cargo subcommand` `cargo ct-adapter new` that scaffolds the boilerplate.  
  → **New:** *Adapter versioning* – embed a semantic version in the adapter ABI to prevent mismatches.

---

## 6. Dashboard, Filters & Grouping (Future Desktop UI)

**Goal:** Manage thousands of sessions across dozens of projects without losing track.

- **Semantic Search (Local-First):** Use a lightweight local embedding model (e.g., `candle` or `rust-bert`) to index user prompts and agent summaries. Search: *"Find the session where I asked about the Stripe integration bug."*  
  → *Expanded:* Index both the user prompt and the agent's final summary / outcome. Allow filtering search results by cost, token volume, success tag, or date range.  
  → **New:** *Fuzzy search* – fallback to TF‑IDF or BM25 for low‑resource environments.

- **Multi-Level Grouping:**  
  - Group by: Project → Agent → Date  
  - Group by: Model → Estimated Cost  
  → *Expanded:* Persist grouping preferences per workspace and support saved *"lenses"* (e.g., *"Expensive failed Claude sessions last 7 days"*).  
  → **New:** *Smart grouping* – automatically suggest groupings based on outliers (e.g., unusually high cost or residual spikes).

- **Success Tagging (Sidecar DB):** Because ContextTrace is read‑only regarding agent logs, user annotations (Success, Failed, Investigating) must be stored in a local ContextTrace SQLite sidecar database, keyed by session ID.  
  → *Expanded:* Allow free‑form notes and linked GitHub issue / Linear ticket IDs. Support bulk tagging from the CLI (`ct tag <id> --status failed --note "hallucinated API"`).  
  → **New:** *Tag analytics* – summarise tag distributions to see what fraction of sessions are successful/failing, and drill into contributing factors.

- **Performance Heatmap:** A grid (GitHub contribution style) showing agent usage. Darker colours = higher token volume or more sessions on that day.  
  → *Expanded:* Click a day to drill into the sessions; hover shows total cost and dominant failure mode for that day.  
  → **New:** *Hourly heatmap* – show usage patterns within a day to identify peak usage times.

---

## 7. Repository Intelligence (Agent-Friendliness)

**Goal:** Understand how *"Agent-Friendly"* your codebase is, based on empirical context usage data.

- **Empirical "Context Magnets":** Don't guess which files are important. Aggregate context data across 50 sessions and definitively state: *"This agent reads src/auth.rs on 90% of turns, but has never once looked at src/utils/formatting.rs."*  
  → *Expanded:* Produce a ranked *"agent attention map"* that can be checked into the repo (or ignored) so new contributors immediately see which files the agents actually care about.  
  → **New:** *File churn vs. context* – correlate file edit frequency with context inclusion to find files that are often changed but rarely seen by the agent.

- **Structure Summarizer:** Generate a high‑level `STRUCTURE.md` for the repo designed specifically for an agent to read, heavily weighting the files that the agent actually uses in practice, rather than just listing the directory tree.  
  → *Expanded:* Also generate a `CONTEXT.md` that lists the top‑N files the agent most frequently needs, with short descriptions extracted from the sessions themselves. This becomes a living, data‑driven alternative to hand‑written agent instructions.  
  → **New:** *Dynamic ignore list* – suggest files/directories to exclude from agent context based on historical usage (files never read).

---

## 8. New Ideas — Developer Experience & Workflow Integration

**Goal:** Make ContextTrace a natural part of a developer's daily agent loop rather than a separate forensic tool.

- **Session Diff (`ct diff`):** Compare two sessions (or two ranges of turns) and highlight structural differences in context composition, tool usage patterns, and residual growth. Useful when *"the agent worked yesterday but fails today on the same task."*  `[→ CT-021]`
  → *Expanded:* Add `--mode summary` for a high‑level overview, and `--mode detailed` for a turn‑by‑turn comparison.

- **Replay-to-Prompt:** Given a turn, reconstruct the exact prompt the model received and offer to re‑send it to a different model (or the same model with different temperature) via the user's existing API keys. Purely local orchestration; never stores keys.  `[→ CT-033]`
  → *Expanded:* Support exporting the prompt as a single file for use with other tools.

- **Agent Behavior Fingerprints:** Derive a compact signature of an agent's tool‑calling and compaction habits across many sessions. Surface anomalies: *"This session used 3× more web‑search tool calls than the median for this project."*  
  → *Expanded:* Allow comparing a single session against the project baseline to spot outliers.

- **Context Budget Advisor:** Before starting a new agent session, run `ct budget .` against recent sessions for the same repo. Output a recommended max‑context strategy or ignore‑list based on historical waste patterns.  
  → *Expanded:* Provide a concrete `claude.json` / `codex.json` config snippet with suggested limits.

- **CI Integration (GitHub Action / local pre‑commit):** A lightweight mode that runs `ct doctor` + fidelity checks on any new session logs committed to a designated directory. Fails the build if recognition rate drops or secret patterns are detected.  
  → *Expanded:* Also run `ct budget` to warn if new sessions exceed historical waste thresholds.

- **Watch Mode:** `ct watch --dir ~/.claude/sessions` that tails new JSONL files and prints a live summary (tokens so far, residual growth, largest tool result) as the agent is still running. Extremely useful for catching context explosions in real time.  
  → *Expanded:* Add a `--alert` flag to trigger desktop notifications when token usage exceeds a threshold.

- **Export to Promptfoo / eval harnesses:** Turn a ContextTrace snapshot into a golden prompt + expected tool‑call sequence that can be fed into evaluation frameworks. Helps teams build regression suites for agent behaviour.  
  → *Expanded:* Also support exporting to `hypothesis` or `pytest` for custom property‑based testing.

---

## 9. New Ideas — Multi-Agent & Cross-Session Intelligence

**Goal:** Move beyond single‑session forensics to insights that only emerge across many sessions or multiple agents.

- **Cross-Session Context Pollution:** Detect when the same large tool result or file appears in many unrelated sessions (e.g., a huge test fixture that keeps being re‑injected). Suggest global ignore rules.  
  → *Expanded:* Provide a `ct polluters` command that lists the top items that appear most frequently and consume the most tokens across sessions.

- **Agent Handoff Analysis:** When a session is continued by a different agent or model (Codex → Claude Code), show exactly which context items survived the handoff and which were lost or re‑estimated.  
  → *Expanded:* Visualise the handoff as a Sankey diagram showing token flow between agents.

- **Team-Level Cost & Quality Dashboard (still local‑first):** Aggregate anonymised metrics across a team's local ContextTrace databases (via optional peer‑to‑peer or shared read‑only SQLite). Answer *"which agent + model combo is cheapest for frontend tasks this month?"*  
  → *Expanded:* Support export of aggregated reports (CSV/JSON) for inclusion in team dashboards.

- **Failure Mode Clustering:** Cluster sessions by the shape of their context just before the agent went off the rails (sudden residual spike, giant tool output, instruction drift). Surface recurring anti‑patterns.  
  → *Expanded:* Use dimensionality reduction (PCA/t‑SNE) on contextual features to produce a 2D map of session failure clusters.

- **Session Family Trees:** Since Claude Code sessions are DAGs of events, reconstruct the full conversation tree, not just the linear sequence. Show branches (e.g., where the agent tried multiple approaches in parallel) and let the user explore each branch independently.  `[→ CT-028]`

- **Context Evolution Hotspots:** Identify turns where the context composition changed the most (largest delta in token distribution). These are often the moments where the agent pivoted strategy or made a significant decision.

---

## 10. New Ideas — Extensibility & Ecosystem

**Goal:** Keep the core tiny while letting power users and researchers build on top of it.

- **Stable Public Domain API:** Publish the `ct-domain` crate with a carefully versioned set of traits and value objects so external tools can consume ContextTrace data without depending on the CLI.  
  → *Expanded:* Version the API independently of the CLI, and provide migration guides for breaking changes.

- **Language Server Protocol (LSP) / Editor Integration:** A lightweight language server that, when a developer is looking at a session log or a `CLAUDE.md`, offers hover info, go‑to‑definition for context items, and *"show me the turn where this file was last in context."*  
  → *Expanded:* Support VS Code, Neovim, and JetBrains via the same LSP backend.

- **Notebook Kernel:** A Jupyter / Observable‑style kernel that lets researchers explore ContextTrace data with Pandas / Polars while keeping the heavy lifting in the Rust core.  
  → *Expanded:* Provide Python bindings via PyO3 and an R interface via extendr.

- **Plugin ABI (already sketched in §5):** Formalise the ABI early and publish a minimal C header + Rust bindgen so adapters can be written in any language that can produce a compatible shared library.  
  → *Expanded:* Provide a testing harness for plugin authors to validate their adapter against a suite of sample logs.

- **WebAssembly (WASM) Target:** Compile the core domain logic to WASM, allowing context inspection directly in the browser (e.g., a static web page that loads a session file and runs entirely client‑side). This is a perfect fit for the local‑first ethos.

---

## Priority Assessment (Revised for Current State)

**Phase 1 (Now - CLI & Core):**  
- `ct-domain` math verification & confidence laundering lint (SARIF)  
- Strictly typed `--json` outputs for `sessions` / `context` / `inspect`  
- Claude Code ACL implementation  
- Residual forensics (`ct residual`)  
- `ct doctor` format‑delta reporting + fidelity score  
- Basic filtering by provenance and size  
- `ct trace` with fuzzy matching and lifecycle view  

**Phase 2 (Validation & Analysis):**  
- Compaction Diffing (Codex) + regret scoring  
- Cost Projection + what‑if scenarios  
- Waste / Low‑Entropy detection  
- DuckDB / Parquet / NDJSON export  
- Session Diff and Watch mode  
- Privacy/secret scanning (local only)  
- Cross‑session pollution detection  

**Phase 3 (Desktop & Advanced UI):**  
- Tauri shell setup  
- Token Treemap + Temporal Scrubbing  
- Ghost Context / multi‑turn overlay  
- Local Semantic Search sidecar  
- Confidence Heatmaps (with SARIF lint integration)  
- Success tagging + sidecar DB  
- Team‑level dashboard (aggregated local)  

**Phase 4 (Ecosystem & Intelligence):**  
- Cross‑session analytics and failure clustering  
- Repository intelligence (context magnets, `STRUCTURE.md`)  
- CI integration and watch mode polish  
- Public domain API + example adapters  
- LSP / editor integration experiments  
- WASM target and notebook kernel

---

## Appendix — MVP Easy Wins (engineering assessment, 2026-07-27)

Assessment against the implemented codebase. The selection criterion is not
"sounds cheap" but **"the domain already carries the data"** — several items
filed above as advanced features turn out to be presentation over state that
already exists.

### Tier A — nearly free, ship in the MVP

| Idea | Why it is nearly free |
|---|---|
| **Calibration drill-down** (§1) | `TokenCount::Calibrated { tokens, raw_estimate }` already stores *both* the pre- and post-scaling figures. The scale factor is `tokens / raw_estimate` — a division, with zero new plumbing. This is the cheapest "insight no other tool has" in the entire backlog. |
| **Strictly typed `--json`** (§2) | Already true by construction. `TokenCount` serialises as `{"kind":"calibrated",…}` and `ContextSource` as `{"origin":…}` because both are `#[serde(tag)]` sum types. Downstream scripts never need to regex strings. Only needs the CLI to emit domain types directly. |
| **Confidence markers in CLI** (§1) | `TokenCount::marker()` and `Confidence::label()` exist. `--confidence-emojis` is a formatting branch. |
| **Raw vs reconciled view** (§5) | Every `Event` carries a `SourceRef`, and `FileRawEventSource::fetch` already returns the exact original bytes. `ct inspect --raw` is wiring, not new capability. |
| **Adapter fidelity score** (§5) | `AgentSession::unrecognised()` already returns the histogram of unmapped event types, collected during parse. The score is `recognised / total`. This is the backbone of `ct doctor` and the early-warning signal for upstream format changes. |

### Tier B — small and high value

- **Residual forensics** (§1) — `ContextSnapshot::residual()` exists per turn; a per-turn series plus a spike threshold is arithmetic. Genuinely novel output, because the residual is *named* rather than smeared.
- **Provenance and size filtering** (§2) — a predicate over `ContextSnapshot::items()`. Directly serves the core "find the 38k-token garbage tool result" workflow.
- **`ct doctor`** (§2) — compose fidelity score + residual spikes + largest contributors. No new domain concepts.
- **Session family trees** (§9) — `EventLinks` already models `parent_uuid` / `logical_parent_uuid` / `is_sidechain`. Branch detection is a graph traversal over data the Claude Code ACL must build anyway. Blocked only on that adapter.

### Tier C — defer, with reasons

- **DuckDB export** (§2) — conflicts with the deliberately minimal dependency tree. `walkdir` and `clap`'s default features were already dropped to keep `windows-sys` out. **`--format ndjson` delivers ~90% of the analyst value for zero new dependencies**; DuckDB can come later behind a feature flag.
- **Replay-to-Prompt** (§8) — re-sending a prompt requires an HTTP client, which **directly violates the "no network-capable crate in the dependency tree" invariant** that makes the local-first guarantee structurally auditable rather than merely promised. If this is ever built it belongs in a separate opt-in crate, never in the default binary. Exporting the prompt to a file (also proposed in §8) achieves the debugging goal with none of the cost.
- **SARIF confidence-laundering lint** (§1) — the invariant is already enforced at compile time by `TokenCount`'s variants and at runtime by `ContextSnapshot`'s balance check. A SARIF pipeline adds CI machinery to re-police something the type system already prevents.
- **Secret scanning** (§4) — requires reading full content of every item, which fights the lazy-`SourceRef` performance design, and a miss creates false confidence. Worth doing properly later, not cheaply now.
- **Semantic search, WASM, LSP, notebook kernel, plugin ABI** — correctly placed in Phases 3–4.

### Revised near-term order

1. Claude Code ACL *(blocks everything else; the corpus is 709 sessions vs 61 for Codex)*
2. Application use cases + `ct sessions` / `ct inspect` / `ct context` / `ct largest`
3. Tier A items — they cost little once the CLI exists
4. `ct doctor` = fidelity score + residual spikes
5. Fixture tests and the zero-panic corpus smoke test
