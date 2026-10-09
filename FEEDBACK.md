# ContextTrace — Comprehensive Engineering Review

## Audit Metadata

- Repository: https://github.com/aneskurtovic/ContextTrace
- Branch: main
- Commit SHA: 8a72cc76a8f1aec04911dc31d48cc63cb3e34d8c
- Audit started: 2026-10-08T19:22:19.385Z
- Last updated: 2026-10-09T00:06:12.280Z
- Audit status: In Progress — broad source review and local validation completed; exhaustive source coverage and desktop/release acceptance remain
- Reviewed environment: Windows; installed Rust/Cargo and Node/npm; offline cached dependencies; approved shell execution; no application launch or real user corpus access
- Reviewer: Astra

## Executive Summary

This revision has a strong layered Rust core, explicit provenance/confidence modeling, bounded JSONL reads, fixture-based compatibility checks, a shared CLI/desktop composition root, and substantial local test coverage. Passing tests nevertheless coexist with important semantic defects. **33 findings are confirmed: 0 Critical, 3 High, 21 Medium and 9 Low.** Confirmation includes traceable source contradictions; selected findings also have synthetic runtime reproductions. It does not imply native reproduction of every issue.

The highest priorities are incorrect request-input membership (CT-101), archive recovery after an interrupted manifest append (CT-104), and heavy synchronous Windows desktop commands (CT-109). Additional practical problems affect incomplete cost accounting, live refresh, browsing unmeasured sessions, default archive redaction and notification fidelity. Archive copies can remain recoverable even when metadata/listing fails; permanent loss of all bytes has not been demonstrated.

Security has useful defenses: evidence-derived file actions, narrowly shaped resume arguments, escaped toast content, restricted webview capabilities and signed updates. Confirmed gaps require explicit interaction with attacker-influenced local evidence or affect the advertised redaction contract; no automatic remote compromise is demonstrated. The npm advisory inventory was refreshed: five flagged development packages require maintenance triage; shipped-app exploitability was not established. Rust advisory inventory remains outstanding.

Workspace tests, strict Clippy, formatting, 180 frontend tests, frontend production build, fixture catalog checks and release-helper unit tests passed locally. Several existing tests encode current behavior rather than independently asserting request-input semantics. Native interaction, assistive technology, real agent restoration, clean-host install/update and current published assets/CI were not verified. This is a substantial audit checkpoint, not an exhaustive production-readiness sign-off. Only FEEDBACK.md is being changed.

## Audit Coverage

Status describes the reviewed scope below. Reviewed means source review; selected passing tests do not elevate an entire subsystem to Verified. Important remaining paths are listed at the end.

| Subsystem | Status | Files Reviewed | Verification | Notes |
|---|---|---|---|---|
| Rust Domain | In Progress | model/session, tokens, context, identity, provenance, notification; ports; calibration, ratio; pricing | Workspace tests; synthetic input/pressure probes | Core evidence/usage semantics inspected; not every model/pricing edge independently rederived |
| Application Services | In Progress | lib, cost, archive, notifications, instructions, transcript, export, secrets; selected corpus/lifecycle/diff/fidelity/diagnostics/ghost/growth/family | Workspace tests; targeted compiled probes | Important workflows traced; exhaustive lifecycle/diff/ghost edge review remains |
| Adapters and Parsing | In Progress | Codex/Claude discovery, parse, reconstruct; jsonl, raw_source, walk, tool_target, archive, pricing, notifications; selected exact/compaction/streams/tokenizers/fingerprint | Workspace tests; 17-fixture manifest; synthetic reconstruction/redaction probes | Request boundary defect established; all producer variants and rewrite races not verified |
| Runtime | Reviewed | src/lib.rs; examples/desktop_perf.rs; desktop AppState cache consumers | Workspace tests; synthetic calibration benchmark | Composition fully inspected; original example not run because it discovers a real corpus |
| CLI | In Progress | main dispatch/load/archive/transcript/export/diff/cost; mcp; format; selected render | Workspace tests; isolated smoke/MCP checks passed | Exhaustive argument/render and malformed MCP protocol checks remain |
| React Frontend | In Progress | App, api, types, demo, format, main, CSS; six test files at relevant behaviors | 180 tests; TypeScript/Vite build | Major state/effect/consumer flows reviewed; nested contracts and all render branches not exhaustive |
| Tauri Integration | Reviewed | lib; commands; resume/local_files; notifications/activation/delivery; config/capabilities; pinned Windows IPC dependencies | Workspace tests, including native command unit tests using stubs | Source boundaries/lifecycle reviewed; installed behavior unverified |
| Security | In Progress | Path actions, resume, instruction reads, redaction/export/archive, MCP transport, CSP/capabilities, update/release integrity | Synthetic redaction test; read-only association evidence; source traces | No exploit executed; advisory inventory, real ACLs and authenticated update acceptance remain |
| Performance | In Progress | Calibration/reconstruction, parsed cache, transcript paging/rendering, notification persistence, corpus; desktop_perf | Synthetic debug derive_ratio timing at three sizes | No production release/RSS/native/soak measurement |
| UI/UX | In Progress | Startup, projects/filtering, workspace/tabs, compare, spend, archives, notifications, resume/update; keyboard/focus logic/CSS | Existing DOM tests; source traces | No visual, screen-reader, geometry, contrast or installed interaction verification |
| Testing | Reviewed | Rust unit/integration/fixture tests; frontend suite; CI smoke helpers; fixture manifest validators; release-helper tests | Commands recorded below actually executed | Test coverage assessed; passing suite does not establish missing scenarios |
| CI and Release | Reviewed | AGENTS, CLAUDE, RELEASING/CI/UPDATER; all five Woodpecker lanes; install/package/publish/gate helpers | Non-publishing helper tests only | No CI dispatch, build packaging, release publication, asset check or clean-host acceptance |
| Cross-Component Flows | Reviewed | Input→snapshot/calibration→IPC; usage→cost→forecast/sandbox; archive→fallback; monitor→cache/event→React; target→native Open; resume/activation | Source traces plus selected synthetic probes | Selected critical flows traced; actual installed end-to-end paths remain |

### Architecture and operational map

ContextTrace 0.1.15 is a local-first Windows x64 desktop application and companion CLI for developers inspecting persisted Codex CLI and Claude Code sessions. Linux core/CLI validation exists; native Linux/macOS desktop acceptance is not established. Ordinary work is discovery→project/session selection→context/usage/transcript/diagnostics→optional comparison/export/archive/resume. Saved stdout/app-server imports have a separate limited timeline contract and must not inherit full persisted-session compatibility claims.

| Layer | Responsibility / boundary |
|---|---|
| ct-domain | Aggregate models, evidence/provenance, nullable usage, compound agent/session identity, ports, calibration/ratio/pricing semantics |
| ct-application | Analysis, transcript, cost/forecast, corpus, lifecycle/diff, instruction comparison, exports, archives and pure notification rules |
| ct-adapters | Codex/Claude anti-corruption layers; bounded JSONL/raw reads; tokenizer/heuristic implementations; filesystem archive/notification storage; public pricing retrieval |
| ct-runtime | Shared agent/estimator/pricing composition; owned archive paths; session calibration helpers |
| ct-cli | Human/JSON command output and stdio MCP; common application use cases, archive fallback |
| ct-ui React/Tauri | Validated IPC DTOs, selection/analysis state, event subscriptions; native file opening, CLI launch, notifications/activation, updater |

Discovery recursively walks configured agent homes (Codex sessions and archived_sessions; Claude projects/subagent layouts), skips symlink entries and bounds depth. Headers provide descriptors; full JSONL parsing preserves source offsets and reports malformed/unsupported/oversized evidence. Claude context follows UUID parent chains and request grouping; Codex replays context items and compaction history. CT-101 concerns the input/completion boundary in both strategies.

Original session inputs are opened read-only. Explicit archive/export and derived notification/pricing/corpus data are placed under a ContextTrace-owned archive root. Missing-source sessions can load archived copies. Preference visibility is reversible; project identity and display labels are separate. Unknown cost/usage is modeled separately in Rust, though CT-102/113 show downstream violations.

Startup composes runtime, archive/cache and notification state, registers single-instance/deep-link/opener/notification/updater plugins, queues activation until React subscribes, and starts a polling thread. The monitor has no explicit cooperative shutdown/join; process exit ends it. Do not infer a shutdown deadlock solely from a detached thread. Frontend effects generally dispose listeners and guard async selection generations, with CT-108/116 exceptions.

Network use includes public LiteLLM pricing/history and GitHub update infrastructure. Ordinary parsing does not upload session content. Resume starts external installed agent CLIs which may write/contact services. Instruction comparison currently permits an unintended network filesystem path (CT-111). Webview CSP limits connections to IPC, and plugin capabilities expose core/updater rather than general filesystem/shell privileges.

Tests are embedded Rust units plus public fixture integration, React/Vitest DOM tests, PowerShell CLI/archive/redaction smoke helpers, and release-gate helper tests. Woodpecker runs Linux core/MSRV, frontend, Windows validation, exact-commit release gates and trusted Windows tag packaging/publication. Local validation, Woodpecker CI, packaging/signature/assets, installed native acceptance and public release are distinct evidence states.

## Findings Summary

| ID | Severity | Category | Finding | Status | Confidence |
|---|---|---|---|---|---|
| CT-101 | High | Data fidelity / reconstruction | Response output is included in the prompt snapshot for the request that generated it | Confirmed | High |
| CT-102 | Medium | Cost correctness | Incomplete billing usage is silently priced as complete | Confirmed | High |
| CT-103 | Medium | State synchronization | Disabling notifications disables live session refresh | Confirmed | High |
| CT-104 | High | Archive reliability | An interrupted manifest append hides the next successful archive | Confirmed | High |
| CT-105 | Medium | Archive transactional integrity | Archive replacement is committed before its metadata | Confirmed | High |
| CT-106 | Medium | Cross-component availability | Sessions without prompt usage cannot open their readable conversation | Confirmed | High |
| CT-107 | Medium | Product calculation | Token Diet labels a fraction of whole-session spend as per-turn savings | Confirmed | High |
| CT-108 | Medium | React async state | A late export result is displayed under a different selected session | Confirmed | High |
| CT-109 | High | Desktop responsiveness / architecture | Heavy synchronous commands run on the Windows event thread | Confirmed | High |
| CT-110 | Medium | Security / native file execution | Open treats Windows Control Panel modules as ordinary data files | Confirmed | High |
| CT-111 | Medium | Security / filesystem trust boundary | Instruction-file comparison can read untrusted network or device paths | Confirmed | High |
| CT-112 | Medium | UI error recovery / stale data | Failed turn comparisons hide recovery controls or retain an obsolete result | Confirmed | High |
| CT-113 | Medium | Evidence presentation | Session comparison ranks unrecorded prompt usage as zero | Confirmed | High |
| CT-114 | Medium | Navigation synchronization | Project options do not refresh with the session catalog | Confirmed | High |
| CT-115 | Medium | Diagnostics fidelity | Overview misattributes session-wide secret findings to the selected turn | Confirmed | High |
| CT-116 | Low | React lifecycle | Asynchronous listener acquisition can leak subscriptions | Confirmed | High |
| CT-117 | Low | UX documentation | Archive guidance incorrectly says saved copies cannot be read | Confirmed | High |
| CT-118 | Medium | Privacy / redaction | Archive redaction loses JSON credential key/value context | Confirmed | High |
| CT-119 | Medium | Context metric correctness | Pressure analysis reuses session limits across requests and model switches | Confirmed | High |
| CT-120 | Medium | Notification replay / cursor semantics | Historical secret and residual findings replay after an unrelated append | Confirmed | High |
| CT-121 | Medium | Cross-agent identity | Global notification deduplication omits agent identity | Confirmed | High |
| CT-122 | Medium | Discovery / recency / filtering | Codex recent activity is fixed to the session creation timestamp | Confirmed | High |
| CT-123 | Medium | Performance / scaling | Claude calibration repeatedly reconstructs the whole session for every turn | Confirmed | High |
| CT-124 | Low | CLI / MCP input correctness | MCP numeric parameters silently wrap instead of rejecting out-of-range values | Confirmed | High |
| CT-125 | Low | Session identity | Family grouping merges independent agents with equal session IDs | Confirmed | High |
| CT-126 | Low | Data fidelity | Growth summaries discard additional compactions assigned to one turn | Confirmed | High |
| CT-127 | Low | CLI contract | Archive verification ignores the accepted JSON flag | Confirmed | High |
| CT-128 | Medium | Cost presentation | Human cost comparisons conceal unpriced usage | Confirmed | High |
| CT-129 | Low | Security / terminal output | CLI inspection prints untrusted terminal controls from metadata | Confirmed | High |
| CT-130 | Low | CLI error communication | An entirely unreadable corpus is reported as empty | Confirmed | High |
| CT-131 | Medium | Parser reliability / resource amplification | Cyclic Claude ancestry amplifies tiny logs into duplicate context | Confirmed | High |
| CT-132 | Low | Parser fidelity | Codex framing-budget edge silently hides oversized messages | Confirmed | High |
| CT-133 | Medium | Parser / product data fidelity | Claude multi-tool blocks lose operation identities and errors | Confirmed | High |

## Critical Findings

None confirmed in this audit. This is not a guarantee that no critical defect exists in remaining or environment-dependent paths.

## High Priority Findings

### CT-101 — Response output is included in the prompt snapshot for the request that generated it

**Severity:** High  
**Category:** Data fidelity / reconstruction  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Codex and Claude reconstructors; context composition, calibration, residuals, largest contributors, lifecycle/diff/export consumers.  
**File Locations:** `crates/ct-adapters/src/codex/parse.rs:1277-1306`; `crates/ct-adapters/src/codex/reconstruct.rs:34-96,346-356`; `crates/ct-adapters/src/claude_code/parse.rs:644-705`; `crates/ct-adapters/src/claude_code/reconstruct.rs:54-87,120-153`; `crates/ct-domain/src/services/calibration.rs:74-80,111-143`.  
**User Impact:** The central “what was in the prompt” view attributes tokens to output that did not yet exist, understates unlogged input and may shrink legitimate prompt contributors. Balanced totals do not make membership correct.  
**Description:** A turn is explicitly modeled as one model request. Codex anchors it at the ending token report, then replays every context-occupying item through that report. Claude anchors at the last assistant block of that request and walks inclusively from it. Both include the current response while using that request’s input-token total as the denominator.  
**Evidence:** Codex regression `replay_includes_everything_up_to_the_anchor_and_nothing_after` explicitly expects user plus current assistant at the first token report. Claude starts its ancestor cursor at the assistant anchor and includes all its context blocks; same-request blocks are grouped into the same turn. Neither path excludes generated response content. Calibrator fits this enlarged list into prompt-only usage.  
**Root Cause:** Completion/report anchors are used as request-input boundaries.  
**Reproduction / Verification:** Independently traced both parsers, reconstructors, consumers and calibration. Existing unit test demonstrates current membership but is not a regression asserting correct semantics. A fixture-only compiled stdin probe ran successfully: both agents included one current ModelOutput item in the first-request input snapshot. No producer or desktop runtime reproduction has been run. A synthetic first request with user U, assistant A and input usage should reconstruct U, never A; a following request should include A when retained.  
**Recommended Solution:** Represent input and completion boundaries separately. For Claude start before the earliest block belonging to the current request on its parent chain; for Codex identify the response boundary using available request/task markers and response item sequence. Preserve past responses and tool results used by subsequent requests. If the precise boundary is not observable, lower membership confidence and disclose uncertainty instead of claiming observed input. Add both-agent tests for single/multi-block responses, tool loops, compaction and branch rewinds, then review ratio/lifecycle baselines.  
**Expected Benefit:** Correct prompt provenance and meaningful attribution/residuals.  
**Estimated Effort:** Large; parser/domain boundary change plus semantic regressions.  
**Regression Risk:** High; nearly every context analysis depends on these snapshots.  
**Related Findings:** None yet.


### CT-104 — An interrupted manifest append hides the next successful archive

**Severity:** High  
**Category:** Archive reliability  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Filesystem archive and missing-source recovery  
**File Locations:** `crates/ct-adapters/src/archive.rs:66-80,323-354,358-387`; `crates/ct-application/src/archive.rs`  
**User Impact:** A successful archive can disappear from listings and fallback recovery after a prior interrupted append. Copy bytes remain on disk but normal recovery has no manifest entry.  
**Description:** Readers skip malformed manifest lines, but writers append directly without repairing or separating an unterminated corrupt tail. An interrupted JSON fragment followed by a valid new entry becomes one malformed line.  
**Evidence:** append_to_manifest uses append(true), serializes JSON plus newline and writes immediately; entries splits by newline and skips serde parse failures. No tail repair exists.  
**Root Cause:** Read-side tolerance was treated as write-side crash recovery.  
**Reproduction / Verification:** Isolated compiled Rust probe on Windows created a new ignored target directory and synthetic source. After seeding manifest.ndjson with `{partial`, ingest returned Ok, entries returned empty and path confirmed the copy existed. Exit 0; no real archive accessed.  
**Recommended Solution:** Under an archive-wide interprocess lock, detect and quarantine/truncate an incomplete tail before appending. Ensure a valid record boundary, flush/sync according to the durability guarantee, and add restart-after-partial-tail regressions for new and existing IDs. Provide an orphan-copy reconciliation path.  
**Expected Benefit:** A failed prior write cannot invalidate the next successful operation.  
**Estimated Effort:** Medium  
**Regression Risk:** Medium; retain manifest history and compatibility.  
**Related Findings:** CT-105

### CT-109 — Heavy synchronous commands run on the Windows event thread

**Severity:** High  
**Category:** Desktop responsiveness / architecture  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Tauri command facade, parsing/analysis, corpus and pricing  
**File Locations:** `crates/ct-ui/src-tauri/src/commands.rs:3131-3150,3229-3235,3265-3273,3324-3337`; `crates/ct-adapters/src/pricing.rs:35-65`  
**User Impact:** Cold session loads, corpus scans and price lookups can block native event handling, delaying interaction and window responsiveness.  
**Description:** Every expensive command is plain #[tauri::command] fn, with no async or worker dispatch. Blocking file analysis and HTTP execute inline. Comments asserting Tauri dispatches these commands to separate threads are incorrect.  
**Evidence:** Pinned cached tauri-macros 2.6.3 wrapper.rs:431 directly calls the handler; Wry 0.55.1 WebView2 custom-protocol callback invokes it inline (mod.rs:953-1027), through tauri-runtime-wry 2.11.4 and tauri 2.11.5 IPC. Both custom-protocol and WebMessageReceived paths were checked. [Tauri command documentation](https://v2.tauri.app/develop/calling-rust/#async-commands) confirms synchronous commands run on the main thread. Pricing has a blocking client with a 12-second per-request timeout.  
**Root Cause:** Assumed dispatch isolation that the command declarations do not request.  
**Reproduction / Verification:** Source and pinned dependency dispatch verified independently by coordinator and desktop reviewer. No native freeze duration measured; this is a confirmed scheduling defect, not a claimed timing benchmark.  
**Recommended Solution:** Use async command facades with owned/Arc state and bounded spawn_blocking jobs for filesystem/CPU/blocking HTTP work. Add cancellation/coalescing where useful, preserve generation guards and short cache locks. Correct misleading comments. Before enabling concurrency, address archive/export scratch names and commit ordering. Verify synthetic long-session and delayed-price operations leave window controls responsive.  
**Expected Benefit:** Keeps the native event loop responsive during analysis.  
**Estimated Effort:** Medium to large  
**Regression Risk:** Medium to high; concurrency exposes previously serialized paths.  
**Related Findings:** CT-104, CT-105

## Medium Priority Findings

### CT-102 — Incomplete billing usage is silently priced as complete

**Severity:** Medium  
**Category:** Cost correctness  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Application pricing; CLI, MCP, desktop and corpus/budget consumers  
**File Locations:** `crates/ct-application/src/cost.rs:280-303,376-396,475-502`; `crates/ct-adapters/src/codex/parse.rs:1223-1244`  
**User Impact:** Missing output or input values become zero-dollar buckets while forecasts can remain enabled.  
**Description:** The usage gate accepts known prompt totals even if output is absent, and accepts output-only records even if input is absent. Cache completeness is checked only when fresh input is known and greater than zero.  
**Evidence:** `usage_cost` unwraps every absent bucket to zero. Accepted turns are marked priced=true and excluded from the unpriced list; forecast only checks that list and quote coverage. Parsers deliberately preserve missing fields.  
**Root Cause:** No explicit billing completeness predicate.  
**Reproduction / Verification:** Source path independently traced. With a deterministic provider, input=100/cache_read=0/output=None is accepted as fully priced. Output=50 with input/cache absent is also accepted. A synthetic compiled probe reproduced both cases as priced=1, unpriced=0, forecast=true. No new test source files created.  
**Recommended Solution:** Require output and applicable input/cache buckets to be known before declaring a complete price; distinguish known zero, inapplicable cache writes and unknown. Exclude incomplete usage from complete forecasts; optionally expose a labelled partial lower bound. Add absent-vs-zero matrices for both agents.  
**Expected Benefit:** Prevents precise-looking underestimates.  
**Estimated Effort:** Small to medium  
**Regression Risk:** Medium; old synthetic tests may encode incomplete usage.  
**Related Findings:** None

### CT-103 — Disabling notifications disables live session refresh

**Severity:** Medium  
**Category:** State synchronization  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Notification monitor, parsed-session cache and Follow live  
**File Locations:** `crates/ct-ui/src-tauri/src/commands/notifications.rs:208-217,284-290,427-439`; `crates/ct-ui/src-tauri/src/commands.rs:260-318`; `crates/ct-ui/src/App.tsx:4892-4919`  
**User Impact:** Follow live can show a frozen session with no explanation when notifications are turned off.  
**Description:** The notification monitor is also the only automatic session invalidation/event publisher. Its master enabled flag stops discovery and cache invalidation before work begins.  
**Evidence:** React Follow live subscribes only to session-updated and has no independent timer or initial refresh. Cached reads do not compare file fingerprints. Explicit forced catalog refresh is a workaround, not live following.  
**Root Cause:** Data change observation is coupled to alert delivery preferences.  
**Reproduction / Verification:** Independently traced monitor→cache→event→React. No installed desktop interaction performed. Disable master notifications, follow a synthetic session, append usage; no event path remains.  
**Recommended Solution:** Keep a lightweight file/catalog change observer running independently of notification settings. Invalidate on change and publish session updates; gate only notification evaluation/delivery. Test live follow with all alerts disabled.  
**Expected Benefit:** Reliable live view regardless of alert preferences.  
**Estimated Effort:** Medium  
**Regression Risk:** Medium; preserve notification baselining/catch-up behavior.  
**Related Findings:** None

### CT-105 — Archive replacement is committed before its metadata

**Severity:** Medium  
**Category:** Archive transactional integrity  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Filesystem archive overwrite and verification  
**File Locations:** `crates/ct-adapters/src/archive.rs:323-354`; `crates/ct-adapters/src/archive.rs:358-387`  
**User Impact:** A failed rearchive can replace a sound prior copy, leaving old digest/redaction metadata attached to new bytes. Verification reports damage even when the new copy is complete.  
**Description:** Destination replacement occurs before append_to_manifest, which can fail. No rollback/versioned copy retains the metadata-byte pairing.  
**Evidence:** fs::rename(tmp,dest) succeeds, then append_to_manifest propagates an I/O error. Existing manifest entry remains selected by last-wins reading.  
**Root Cause:** A two-file commit has no transaction or recovery journal.  
**Reproduction / Verification:** Isolated compiled Rust probe on Windows ingested synthetic version 1, moved its own manifest aside and created a directory at that manifest path to force append-open failure. Reingesting version 2 returned Err after replacing the copy. Restoring the original manifest made verify return ArchiveDamaged with its old digest. Exit 0. Only new ignored test artifacts were renamed; no user data or permissions changed. Content bytes remained recoverable.  
**Recommended Solution:** Write immutable versioned copies named by digest/revision, durably append the reference, and keep the prior referenced copy until success. Add failure-injected manifest-open/write cases and startup reconciliation. Use unique create_new scratch files and root-wide writer coordination for related concurrency risk.  
**Expected Benefit:** Preserves the last acknowledged archive and accurate redaction/integrity status.  
**Estimated Effort:** Medium  
**Regression Risk:** Medium; migration and garbage-collection rules required.  
**Related Findings:** CT-104

### CT-106 — Sessions without prompt usage cannot open their readable conversation

**Severity:** Medium  
**Category:** Cross-component availability  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Tauri context command and React session selection  
**File Locations:** `crates/ct-ui/src-tauri/src/commands.rs:556-612`; `crates/ct-ui/src/App.tsx:4874-4889`  
**User Impact:** New/incomplete or format-drift sessions with valid messages cannot expose transcript, metadata or export/archive controls through the workspace.  
**Description:** inspect_session can return useful metadata/events even with no measured turn. get_context(None) rejects when no peak prompt exists. React requires both promises to succeed before storing detail.  
**Evidence:** Promise.all success is the only assignment to detail/detailFor in selection; a context failure discards the independent successful inspect result.  
**Root Cause:** Optional analysis is a prerequisite for basic browsing.  
**Reproduction / Verification:** Backend error and frontend state path independently traced. The frontend “empty context” test mocks a successful context payload and does not exercise this backend rejection.  
**Recommended Solution:** Load metadata/transcript independently. Model context as unavailable with a clear reason, keeping conversation/archive/export accessible. Add a contract-level fixture for messages with no usage and a deferred failed context request.  
**Expected Benefit:** Useful evidence stays inspectable when metrics are unavailable.  
**Estimated Effort:** Small to medium  
**Regression Risk:** Low to medium  
**Related Findings:** CT-101

### CT-107 — Token Diet labels a fraction of whole-session spend as per-turn savings

**Severity:** Medium  
**Category:** Product calculation  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** React what-if sandbox and cost report  
**File Locations:** `crates/ct-ui/src/App.tsx:1580-1588`; `crates/ct-application/src/cost.rs:386-429`  
**User Impact:** Estimated savings increase with unrelated historical turns and include output/cache charges that dropping an input item does not necessarily avoid.  
**Description:** perToken divides whole-session cost.total by one selected prompt total, then multiplies removed input tokens and displays dollars / turn.  
**Evidence:** For identical selected prompts, a session costing $100 instead of $1 makes the same removed item show 100× savings. The backend report total is summed across priced turns.  
**Root Cause:** Session-wide spend and request input dimensions are mixed.  
**Reproduction / Verification:** Formula independently checked; no UI execution. Simulation badge does not disclose the invalid rate basis.  
**Recommended Solution:** Use the selected request’s applicable marginal input/cache rate with explicit assumptions, excluding output and unrelated turns. If a reliable rate is unavailable, show token savings only. Test invariance when earlier turns are added.  
**Expected Benefit:** Actionable savings estimates that reflect the selected request.  
**Estimated Effort:** Small to medium  
**Regression Risk:** Low  
**Related Findings:** CT-102

### CT-108 — A late export result is displayed under a different selected session

**Severity:** Medium  
**Category:** React async state  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Export selected handler and session selection  
**File Locations:** `crates/ct-ui/src/App.tsx:4860-4873,5370-5383`  
**User Impact:** After switching A→B during export, A’s path/error can appear in B’s export panel and reset the loading state of a newer export.  
**Description:** Selection clears export state but does not invalidate in-flight export promises. exportSelected writes result, error and busy state unconditionally.  
**Evidence:** Other detail/analysis requests use generation counters; export handler has none. The selected object captured by the callback cannot prevent late state mutation.  
**Root Cause:** Operation results lack session and request identity.  
**Reproduction / Verification:** Source-confirmed deferred-response ordering; no production export performed.  
**Recommended Solution:** Track export request generation plus agent/id, invalidate on selection/unmount, and condition all success/error/finally updates. Alternatively store outcomes keyed by session. Add deferred A export→select B→resolve A regression, including overlapping B export.  
**Expected Benefit:** Prevents misattributed export results.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** None


### CT-110 — Open treats Windows Control Panel modules as ordinary data files

**Severity:** Medium  
**Category:** Security / native file execution  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Evidence-derived local file actions and OS associations  
**File Locations:** `crates/ct-ui/src-tauri/src/commands/local_files.rs:116-126,158-159,219`; `crates/ct-ui/src/App.tsx:1215-1216`  
**User Impact:** A user clicking a logged local .cpl target can execute code through Control Panel, despite the explicit executable-file opening restriction.  
**Description:** Executable extension denylist omits .cpl. Existing regular local CPL files receive canOpen=true and reach the OS default association.  
**Evidence:** Reviewed cached opener 2.5.3 enables open 5.4.4 ShellExecuteExW dispatch. Read-only registry verification by specialist found .cpl→cplfile→control.exe. [Microsoft documentation](https://learn.microsoft.com/en-us/windows/win32/shell/control-panel-applications) describes CPL DLL modules executed as Control Panel items. Canonical-path and UNC guards do not address this type.  
**Root Cause:** A partial denylist is used as a data-versus-active-content boundary.  
**Reproduction / Verification:** Attacker must place a malicious CPL locally, get its target into a session (e.g. malicious workspace/tool output) and induce an explicit Open click. No automatic execution, malicious file creation or payload execution was performed. Source acceptance is confirmed; exploit execution intentionally untested.  
**Recommended Solution:** Prefer an allowlist of supported data types with Reveal-only handling for unknown/active types; at minimum cover CPL and review other Windows executable associations. Recheck canonical target and action at dispatch. Add canOpen=false tests for active formats and symlink targets.  
**Expected Benefit:** Makes file Open honor the intended executable-content boundary.  
**Estimated Effort:** Small to medium  
**Regression Risk:** Medium; some legitimate file types become Reveal-only.  
**Related Findings:** CT-111

### CT-111 — Instruction-file comparison can read untrusted network or device paths

**Severity:** Medium  
**Category:** Security / filesystem trust boundary  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Instruction comparison in desktop, CLI and MCP  
**File Locations:** `crates/ct-application/src/instructions.rs:105-117,186-194`; `crates/ct-adapters/src/claude_code/parse.rs:543-565`; `crates/ct-ui/src-tauri/src/commands.rs:1138-1141`; `crates/ct-cli/src/mcp.rs:270-275`  
**User Impact:** Comparing instructions can cause remote filesystem access or unbounded/special-file reads based on session metadata. Windows SMB authentication is a conditional risk.  
**Description:** A nested_memory attachment label is treated as an OS path. resolve_instruction_path accepts absolute UNC/device paths and drive-relative names; std::fs::read has no locality, regular-file or size check.  
**Evidence:** Untrusted local-session attachment→parser label→content-analysis comparison→direct read crosses from evidence into OS filesystem access. File-opening IPC has a locality policy, but this independent read path does not. [Microsoft SMB authentication documentation](https://learn.microsoft.com/en-us/windows/win32/fileio/microsoft-smb-protocol-authentication) supports possible authentication on network access.  
**Root Cause:** Path validation is inconsistent across features; application analysis performs concrete filesystem I/O.  
**Reproduction / Verification:** Requires a crafted/imported/modified session plus explicit instruction comparison in desktop/CLI or an MCP call. Source path acceptance confirmed. No network/share/device access or credential transmission attempted; actual authentication depends on policy and reachability.  
**Recommended Solution:** Introduce a common local-path policy and injected instruction-file reader. Reject UNC/device/drive-relative paths before I/O, canonicalize legitimate local paths, bound regular-file reading/hashing, and return typed refusal. Preserve legitimate home/repo instruction paths. Add malicious attachment path regressions without contacting a share.  
**Expected Benefit:** Prevents session evidence from silently expanding filesystem/network authority.  
**Estimated Effort:** Medium  
**Regression Risk:** Medium; support legitimate user-level instruction locations.  
**Related Findings:** CT-110

### CT-112 — Failed turn comparisons hide recovery controls or retain an obsolete result

**Severity:** Medium  
**Category:** UI error recovery / stale data  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Compare view, endpoint request and result state  
**File Locations:** `crates/ct-ui/src/App.tsx:644-661,5281-5313`  
**User Impact:** Entering an unavailable turn can remove the editable comparison section; after prior success, the old comparison remains while the picker names new endpoints.  
**Description:** Failure is recorded only in a global banner. With no result, loading stops and TurnComparison returns null; with a result, the effect never clears or marks it stale for new endpoints.  
**Evidence:** Picker is rendered in loading/no-endpoint branches but omitted at if(!diff)return null. Request generation prevents late replies but does not associate the visible result with requested endpoints.  
**Root Cause:** Missing local error state and result endpoint identity.  
**Reproduction / Verification:** Source transition traced. No native or browser interaction claimed.  
**Recommended Solution:** Keep picker visible for loading/error/empty/success, show an inline error and retry, and bind each result to exact endpoint identities. Clear or explicitly mark prior results while requests change. Test first-request and replacement failure.  
**Expected Benefit:** Users can correct input without losing comparison context.  
**Estimated Effort:** Small to medium  
**Regression Risk:** Low  
**Related Findings:** CT-108

### CT-113 — Session comparison ranks unrecorded prompt usage as zero

**Severity:** Medium  
**Category:** Evidence presentation  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** A/B session comparison  
**File Locations:** `crates/ct-ui/src/App.tsx:1669-1677`; `crates/ct-ui/src-tauri/src/commands.rs:565-580`  
**User Impact:** An unmeasured candidate appears to use fewer peak tokens than a measured session.  
**Description:** Both nullable peak measurements are coerced to zero, then an exact numeric delta is shown.  
**Evidence:** Backend preserves None. Candidate selection invokes only inspectSession, so CT-106 does not prevent an unmeasured candidate from reaching the panel.  
**Root Cause:** Unknown and measured zero are conflated in presentation.  
**Reproduction / Verification:** Source independently checked; a null candidate against a 20k-token current session displays 20k fewer.  
**Recommended Solution:** Render Not recorded for null and withhold delta unless both sides are measured. Add one-sided and both-sided null regressions.  
**Expected Benefit:** Prevents misleading experiment conclusions.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** CT-106

### CT-114 — Project options do not refresh with the session catalog

**Severity:** Medium  
**Category:** Navigation synchronization  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Project picker and catalog refresh  
**File Locations:** `crates/ct-ui/src/App.tsx:4529-4572,4775-4777`  
**User Impact:** New projects remain absent and counts/deleted project options stay stale after pressing Refresh.  
**Description:** Project effect depends only on filter changes, while refreshSessions updates catalog rows and totals independently.  
**Evidence:** The project API is not called by forced refresh. Changing an unrelated filter is the only normal way to reload options.  
**Root Cause:** Catalog and project summaries have separate invalidation generations.  
**Reproduction / Verification:** Source effect/caller trace confirmed; no runtime discovery performed.  
**Recommended Solution:** Refresh project options and sessions under a shared catalog generation, preserving stale-response guards and same query/filter semantics. Test changed project set with unchanged filters.  
**Expected Benefit:** A refreshed catalog has matching navigation options.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** CT-103

### CT-115 — Overview misattributes session-wide secret findings to the selected turn

**Severity:** Medium  
**Category:** Diagnostics fidelity  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Doctor result and Overview diagnostics rail  
**File Locations:** `crates/ct-ui/src-tauri/src/commands.rs:772-800`; `crates/ct-ui/src/App.tsx:1394-1399`  
**User Impact:** Users are directed to inspect the wrong turn when a credential-shaped value occurs elsewhere.  
**Description:** Doctor secretOccurrences counts a scan of the entire cached session. Overview interpolates the currently selected context.turn into its description of that aggregate count.  
**Evidence:** Detailed SecurityDashboard correctly lists actual finding turn/line, so this is a contradictory overview label, not secret-value exposure.  
**Root Cause:** A session-scoped scan is combined with a turn-scoped label.  
**Reproduction / Verification:** Source scope/consumer independently traced.  
**Recommended Solution:** Label aggregate secret results session-wide or filter actual finding.turn before making a selected-turn assertion. Test a finding only outside the selected turn.  
**Expected Benefit:** Accurate triage location and consistent diagnostic scope.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** None

### CT-118 — Archive redaction loses JSON credential key/value context

**Severity:** Medium  
**Category:** Privacy / redaction  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** RedactingTransform and archive ingestion  
**File Locations:** `crates/ct-application/src/archive.rs:72-95`; `crates/ct-application/src/secrets.rs:643-706`  
**User Impact:** An archive labelled Redacted can retain a credential that the session scanner recognizes.  
**Description:** The scanner recognizes generic credential assignments in a whole raw JSON record, but archive transformation independently scans each JSON string span. A credential key and its ordinary-looking value are separated, removing the assignment context needed for recognition.  
**Evidence:** A programmatic JSON password assignment was recognized by redact_preview on the complete record; RedactingTransform returned the exact unchanged record and zero replacements. No secret value was printed.  
**Root Cause:** Structure-preserving string redaction changed the scanner input contract while documentation still claims equivalent detection.  
**Reproduction / Verification:** Compiled stdin probe ran successfully using existing libraries and a synthetic value, with scanner_detected=true, archive_unchanged=true, replacements=0. No private archive was touched.  
**Recommended Solution:** Preserve JSON member context when detecting generic credential values and redact only the corresponding value span. Decode escaped strings for detection with a safe mapping back to encoded spans. Retain JSON validity and byte identity for unaffected records. Add nested generic-key, escaped-value and punctuation-preservation regressions. Do not promise removal of arbitrary unknown secrets.  
**Expected Benefit:** Makes the documented archive redaction guarantee consistent with scanner detection.  
**Estimated Effort:** Medium  
**Regression Risk:** Medium; preserving valid JSON and original unaffected bytes is essential.  
**Related Findings:** CT-115

### CT-119 — Pressure analysis reuses session limits across requests and model switches

**Severity:** Medium  
**Category:** Context metric correctness  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Notification pressure, growth timeline and corpus pressure  
**File Locations:** `crates/ct-application/src/notifications.rs:156-163,254-258`; `crates/ct-domain/src/model/session.rs:251-265`; `crates/ct-application/src/growth.rs:122-125,261`; `crates/ct-application/src/corpus.rs:367-390`  
**User Impact:** A request with unknown context capacity can produce a precise-looking critical pressure alert using another model’s limit.  
**Description:** Notification utilisation and displayed evidence fall back directly to session metadata. Growth and corpus pressure use only the session-wide capacity, even when a later request has an explicit different limit. The domain helper deliberately rejects that fallback when the request explicitly changes model.  
**Evidence:** Source trace also shows metadata 200k with a later 80k/100k request produces corpus comfortable (40%) rather than tight (80%). A synthetic model-B request with 90,000 prompt tokens and only model-A session metadata (100,000 capacity) has context_window_at=None but produces a pressure candidate.  
**Root Cause:** Multiple analyses duplicate capacity resolution instead of using the domain evidence policy.  
**Reproduction / Verification:** Corrected isolated Rust probe with notifications enabled passed; no OS notification sent.  
**Recommended Solution:** Use context_window_at for each request in notifications, growth and corpus. Compute the maximum known per-turn utilization rather than peak tokens divided by one session limit; keep unknown-denominator requests explicit. Growth ranges must retain the included requests’ limits. Test model switches, explicit per-request limits and zero limits.  
**Expected Benefit:** Consistent uncertainty across the context view and alerts.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** CT-101

### CT-120 — Historical secret and residual findings replay after an unrelated append

**Severity:** Medium  
**Category:** Notification replay / cursor semantics  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Monitor baselining, whole-session deep analysis, notification rules  
**File Locations:** `crates/ct-ui/src-tauri/src/commands/notifications.rs:295-302,345-358`; `crates/ct-application/src/notifications.rs:555-569,709-725,749-767`  
**User Impact:** Previously existing issues can appear as newly generated feed entries or OS alerts after monitoring starts, despite the no-history baseline.  
**Description:** Startup saves a cursor without evaluating/inserting historic secret or residual keys. On the next file change the monitor supplies whole-session scans; evaluate_deep filters instruction changes by prior turn but pushes all secret findings and residual steps.  
**Evidence:** Coordinator verified baseline early return, full-session input construction and lack of filtering. Existing global dedupe only helps after an issue has already been inserted.  
**Root Cause:** Deep rules do not apply the same cursor boundary as events, snapshots and instruction drift.  
**Reproduction / Verification:** Synthetic first-turn baseline followed by an unrelated second turn generated a secret candidate for turn 1. First attempt had notifications disabled (default) and returned none; corrected enabled-settings probe passed. Residual replay is source-confirmed through the same unfiltered loop, not separately executed.  
**Recommended Solution:** Filter deep findings against prior sequence/source-line and turn identity, including findings without a turn. Alternatively seed stable deep keys during baseline without emitting alerts. Define rotation/restart semantics explicitly. Add old secret and residual baseline→append tests and a genuinely new finding control.  
**Expected Benefit:** Prevents misleading historical replay while preserving new alerts.  
**Estimated Effort:** Medium  
**Regression Risk:** Medium; cursor filtering must not suppress new findings in an existing turn.  
**Related Findings:** CT-103, CT-119

### CT-121 — Global notification deduplication omits agent identity

**Severity:** Medium  
**Category:** Cross-agent identity  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Notification candidate keys and durable dedupe store  
**File Locations:** `crates/ct-application/src/notifications.rs:287,344,449,503,720-725,761,832`; `crates/ct-adapters/src/notifications.rs:212-226`  
**User Impact:** When both agents have the same session ID, one agent’s matching alert can suppress the other’s.  
**Description:** Most session-scoped keys contain session ID and rule evidence but omit agent. The durable dedupe-key vector is global, although session location and checkpoints correctly use agent plus ID.  
**Evidence:** Synthetic Codex and Claude sessions with identical IDs and equivalent secret finding produce equal dedupe keys. Store insertion rejects a key already present independently of location.  
**Root Cause:** Compound session identity was not carried into the notification namespace.  
**Reproduction / Verification:** Compiled isolated probe asserted equal cross-agent keys. No real notification state was read or written. Collision is an edge case; random producer UUIDs usually differ, but identical IDs are supported elsewhere in the application.  
**Recommended Solution:** Centralize key generation with a versioned agent/id namespace for session-scoped rules. Preserve deliberate cross-session format-drift dedupe separately. Plan migration so existing keys do not cause an alert replay storm. Add same-ID/different-agent store regression.  
**Expected Benefit:** Consistent session identity and independent alerts.  
**Estimated Effort:** Small to medium  
**Regression Risk:** Medium; persisted dedupe migration needs care.  
**Related Findings:** CT-120

### CT-122 — Codex recent activity is fixed to the session creation timestamp

**Severity:** Medium  
**Category:** Discovery / recency / filtering  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Codex descriptor, catalog sorting and date filters  
**File Locations:** `crates/ct-adapters/src/codex/mod.rs:195-196`; `crates/ct-application/src/lib.rs:222,262,1096-1103`; `crates/ct-ui/src-tauri/src/commands/notifications.rs:259-273`  
**User Impact:** Resuming an old Codex session does not move it to the recent position and a since-date filter can exclude newly active work.  
**Description:** Discovery assigns both started_at and last_activity from the session header timestamp rather than file activity. The shared catalog sorts and filters by last_activity.  
**Evidence:** Descriptor construction and sort/filter consumers directly contradict the meaning of last_activity. Normal appends still change size and are noticed by the monitor; same-size rewrites do not change this descriptor fingerprint.  
**Root Cause:** Creation and activity timestamps are conflated in Codex discovery.  
**Reproduction / Verification:** Source-confirmed end-to-end path; no user session discovery or filesystem timestamp manipulation performed.  
**Recommended Solution:** Keep header timestamp as started_at; use a documented cheap activity signal (such as filesystem modified time, with explicit fallback/limitations) for descriptor recency. Add an isolated old-created/recently-modified discovery fixture and since/sort tests.  
**Expected Benefit:** Resumed sessions remain discoverable as recent work.  
**Estimated Effort:** Small  
**Regression Risk:** Low to medium; define copy/restore timestamp semantics.  
**Related Findings:** CT-114

### CT-123 — Claude calibration repeatedly reconstructs the whole session for every turn

**Severity:** Medium  
**Category:** Performance / scaling  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Session ratio derivation, Claude reconstruction, desktop cache misses  
**File Locations:** `crates/ct-application/src/lib.rs:780-799`; `crates/ct-adapters/src/claude_code/reconstruct.rs:56-87,96-103`; `crates/ct-ui/src-tauri/src/commands.rs:282`  
**User Impact:** Long sessions incur rapidly growing cold-load analysis work; invalidation repeats this work. On the current synchronous desktop path it contributes to blocked interaction.  
**Description:** derive_ratio reconstructs each measured turn. Every reconstruction rebuilds an all-event UUID index, walks its ancestor chain, constructs items and sums them. This is at least O(T×E), and a growing linear history also repeats prefixes quadratically.  
**Evidence:** An isolated debug-build synthetic Claude DAG (two events per turn; short constant-size messages) took 446.92ms at 250 turns/500 events, 1675.30ms at 500/1000, and 6627.46ms at 1000/2000. Doubling turns produced approximately 3.75× and 3.96× elapsed work.  
**Root Cause:** A per-turn reconstruction API is reused for aggregate calibration without session-level indexing or incremental aggregates.  
**Reproduction / Verification:** Compiled stdin benchmark ran existing debug libraries with programmatic input only. These are single-run debug timings for derive_ratio, not production-release, cold-disk, native UI or RSS measurements. No private sessions used.  
**Recommended Solution:** First correct CT-101 boundaries, then build the UUID index once per loaded session and derive chain character/depth totals with memoized branch-aware aggregates respecting compaction/sidechains. Avoid constructing full ContextItem vectors solely for calibration. Bound/sample calibration only if accuracy is demonstrated. Add release-mode scaling benchmarks over synthetic linear, branched and compacted sessions with unchanged ratio assertions.  
**Expected Benefit:** Reduces repeat work and provides a measurable latency budget for long sessions.  
**Estimated Effort:** Medium to large  
**Regression Risk:** Medium to high; branch/compaction/calibration semantics must remain correct.  
**Related Findings:** CT-101, CT-109

### CT-128 — Human cost comparisons conceal unpriced usage

**Severity:** Medium  
**Category:** Cost presentation  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** CLI what-if rendering and cost report coverage  
**File Locations:** `crates/ct-cli/src/render.rs:436-462`; `crates/ct-application/src/cost.rs:231-255,392-393`  
**User Impact:** A comparison can present precise zero totals or misleading savings while excluding unknown-priced turns.  
**Description:** The renderer drops both detailed reports’ warnings and unpriced arrays; generic policy assumptions do not recover that information.  
**Evidence:** Normal cost rendering at render.rs:386-433 explicitly prints warnings and unpriced turns; cost_comparison prints only totals, savings and scenario assumptions.  
**Root Cause:** Coverage information is discarded at the presentation boundary.  
**Reproduction / Verification:** Source-traced report-to-render contradiction. JSON preserves the detailed reports; no current network pricing request required to establish the omission.  
**Recommended Solution:** Render coverage and warnings for both populations, label totals as partial where needed and withhold a savings claim when baseline/scenario priced populations differ. Add all-unpriced and different-coverage cases.  
**Expected Benefit:** Cost scenarios communicate uncertainty as clearly as ordinary cost reports.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** CT-102, CT-107  

### CT-131 — Cyclic Claude ancestry amplifies tiny logs into duplicate context

**Severity:** Medium  
**Category:** Parser reliability / resource amplification  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Claude parent-chain reconstruction and derived analyses  
**File Locations:** `crates/ct-adapters/src/claude_code/reconstruct.rs:120-153,537-548`  
**User Impact:** A malformed two-node cycle creates 100,000 duplicate context items, corrupting provenance and increasing work in context, calibration and IPC consumers.  
**Description:** The ancestry walk rejects self-links but revisits multi-node cycles until a large fixed step cap.  
**Evidence:** Existing cycle regression checks only nonempty termination. No visited-index set prevents repeated members.  
**Root Cause:** A traversal step cap is substituted for graph cycle detection.  
**Reproduction / Verification:** Synthetic two-event AgentSession with a↔b parents reconstructed exactly 100,000 items in a compiled adapter probe (exit 0). No user logs opened; native timing was not measured.  
**Recommended Solution:** Track visited event indices/UUIDs, stop before a repeated member, propagate an incomplete/cyclic ancestry diagnostic and retain each member at most once. Strengthen tests for two/longer cycles, acyclic long chains and branch boundaries.  
**Expected Benefit:** Bounded work proportional to actual unique evidence and honest reconstruction fidelity.  
**Estimated Effort:** Small  
**Regression Risk:** Medium; expose incomplete ancestry without treating it as complete.  
**Related Findings:** CT-101, CT-123  

### CT-133 — Claude multi-tool blocks lose operation identities and errors

**Severity:** Medium  
**Category:** Parser / product data fidelity  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Claude accounting events, corpus metrics and tool-error notifications  
**File Locations:** `crates/ct-adapters/src/claude_code/parse.rs:472-486,519-531`; `crates/ct-application/src/corpus.rs:296-360`; `crates/ct-adapters/tests/fixtures.rs:250-320`; `tests/fixtures/claude_code/chat-blocks.jsonl`  
**User Impact:** Multiple calls/results on one line are counted as one; a later failed result can disappear from error metrics and notifications, and combined sizes attach to the first call.  
**Description:** Transcript blocks preserve all operations, but normalized accounting selects only the first tool_use/tool_result while measuring the entire group.  
**Evidence:** The committed fixture has call-a and call-b, a successful first result and failed second result. Both classification functions use find, and corpus uses those singular event identities/error flags.  
**Root Cause:** One primary display classification doubles as a lossy accounting representation.  
**Reproduction / Verification:** Independently traced committed fixture through parser and corpus; existing tests assert display blocks but not two calls/one error. No real producer incidence or native notification reproduction claimed.  
**Recommended Solution:** Preserve per-block tool identities/error flags in accounting, using separate block facts or derived operation records; retain one request usage record and UUID ancestry and avoid duplicating whole-message size. Assert corpus calls/errors/attribution and notification behavior for this fixture.  
**Expected Benefit:** Consistent transcript, metrics and error reporting for supported multi-block messages.  
**Estimated Effort:** Medium  
**Regression Risk:** Medium; preserve event source/ancestry and usage deduplication.  
**Related Findings:** CT-101  

## Low Priority Findings

### CT-116 — Asynchronous listener acquisition can leak subscriptions

**Severity:** Low  
**Category:** React lifecycle  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Corpus progress effect and multi-listener notification helper  
**File Locations:** `crates/ct-ui/src/App.tsx:2820-2823,4675-4679`; `crates/ct-ui/src/api.ts:1584-1599`; `crates/ct-ui/src/main.tsx:10-15`  
**User Impact:** Fast navigation or partial registration failure can leave orphan callbacks; registration errors in the corpus effect become unhandled rejections.  
**Description:** Corpus cleanup can run before listen resolves and therefore sees no unlisten function. Separately, Promise.all registration of two notification listeners loses the successful cleanup handle if the other registration rejects.  
**Evidence:** Existing session effects already demonstrate a disposed-flag pattern. The notification helper exposes cleanup only after both registrations succeed, and its caller cannot reclaim a handle lost inside the rejected promise.  
**Root Cause:** Asynchronous acquisition has neither disposal reconciliation nor partial-failure rollback.  
**Reproduction / Verification:** Source ordering and failure branches verified by independent frontend reviewer and coordinator; no measured listener leak.  
**Recommended Solution:** Use a disposed flag and immediately close late registrations; catch errors. Acquire notification listeners with explicit rollback or allSettled and dispose successful acquisitions when any fail. Add deferred registration→unmount and one-success/one-failure tests.  
**Expected Benefit:** Bounded subscription lifetime and explicit acquisition errors.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** None

### CT-117 — Archive guidance incorrectly says saved copies cannot be read

**Severity:** Low  
**Category:** UX documentation  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Archive panel and recovery workflow  
**File Locations:** `crates/ct-ui/src/App.tsx:2381-2384,3678-3684`; `crates/ct-application/src/archive.rs`; `crates/ct-cli/src/main.rs:663-797`  
**User Impact:** Users are told recovery is unavailable even though catalog/fallback supports missing-source archives.  
**Description:** Panel claims nothing reads a copy back here or in CLI. Application load_with_archive and CLI readers already support archive fallback and the workspace renders archive source labels.  
**Evidence:** Coordinator verified CLI uses load_with_archive throughout inspect/transcript/context and desktop cached_session uses agent-scoped archive fallback. Archive panel rows may still be non-clickable, which does not justify the broader claim.  
**Root Cause:** Old feature-stage copy remains after recovery implementation.  
**Reproduction / Verification:** Source/documentation contradiction confirmed.  
**Recommended Solution:** Describe actual missing-source fallback and how to locate saved sessions; distinguish non-clickable archive-table rows from supported catalog recovery.  
**Expected Benefit:** Clear recovery guidance.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** CT-104, CT-105


### CT-124 — MCP numeric parameters silently wrap instead of rejecting out-of-range values

**Severity:** Low  
**Category:** CLI / MCP input correctness  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** MCP turn and scenario argument decoding  
**File Locations:** `crates/ct-cli/src/mcp.rs:101-114,168,196,296-304,322-326,410`  
**User Impact:** A client can request one turn and receive another without an error; oversized forecast/cap values are also truncated.  
**Description:** JSON u64 values are cast to u32 using as, which wraps above u32::MAX. Schemas specify minimum values without matching maximums; handler code does not validate range. Wrong-type optional values also silently fall back, an adjacent validation gap.  
**Evidence:** A real MCP subprocess using isolated public-fixture homes returned the exact same successful composition report for turn 4294967297 and turn 1.  
**Root Cause:** Advertised tool schemas and decoded domain ranges are not enforced by checked conversions.  
**Reproduction / Verification:** Executed initialize, initialized notification, ping, tools/list and both context calls. Five valid responses, no response to notification, 17 tools listed, oversized turn aliases turn 1=true. No real sessions or external services used.  
**Recommended Solution:** Centralize checked typed argument decoding, use u32::try_from and TurnNumber validation, reject supplied values of the wrong type/range, and add matching schema maximums. Cover 0, 1, u32::MAX, u32::MAX+1/+2, negatives and strings across turn/forecast/cap arguments.  
**Expected Benefit:** Predictable tool contracts and truthful requested-turn identity.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** None

### CT-125 — Family grouping merges independent agents with equal session IDs

**Severity:** Low  
**Category:** Session identity  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Application family grouping and CLI families  
**File Locations:** `crates/ct-application/src/family.rs:21-64`; `crates/ct-cli/src/main.rs:596-609`; related `crates/ct-application/src/diff.rs:267-268`  
**User Impact:** An uncommon cross-agent ID collision hides one root and can associate branches with the other agent.  
**Description:** families keys root and parent maps by the bare ID string; the unfiltered CLI accepts both agents.  
**Evidence:** roots.insert overwrites equal IDs regardless of agent; branch entries also share the bare parent key.  
**Root Cause:** Compound identity is discarded during grouping.  
**Reproduction / Verification:** Isolated Rust probe passed two synthetic root descriptors with the same ID and different agents; families returned one result (exit 0). Branch merging follows the same map key. No real corpus accessed.  
**Recommended Solution:** Key both maps by (AgentKind, SessionId), carry agent in the family DTO and add mixed-agent root/orphan/branch tests. Include agent in diff same_session identity to prevent misleading explanation text.  
**Expected Benefit:** Preserves independent producer identities and branch provenance.  
**Estimated Effort:** Small  
**Regression Risk:** Low; update DTO consumers and deterministic ordering.  
**Related Findings:** CT-121  

### CT-126 — Growth summaries discard additional compactions assigned to one turn

**Severity:** Low  
**Category:** Data fidelity  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Growth timeline, buckets and CLI/MCP summaries  
**File Locations:** `crates/ct-application/src/growth.rs:127-132,153-161,215-265`; `crates/ct-adapters/src/codex/parse.rs:1277-1306`  
**User Impact:** Multiple recorded compactions before a request report are shown and counted as one. Their triggers/reclaimed amounts are lost from growth output.  
**Description:** The temporary map and GrowthPoint represent only one CompactionAt per turn. Later inserts overwrite prior marks without increasing unplaced_compactions.  
**Evidence:** placed.insert(turn.get(), mark) replaces the prior value; compactions counts points with a mark. Codex grouping assigns every pending event to the next token-report turn, allowing multiple marks on one turn.  
**Root Cause:** A one-to-one representation is used for a many-to-one event relationship.  
**Reproduction / Verification:** Deterministic source path: two Compacted events both at turn 1 produce one placed mark and zero unplaced marks. Producer incidence was not measured; no frequency claim.  
**Recommended Solution:** Retain all marks per turn, or an explicit count plus a documented representative mark; update total, buckets and range calculations. Add two-compactions-one-turn and orphan-mark regressions.  
**Expected Benefit:** Growth summaries preserve all recorded compactions.  
**Estimated Effort:** Small  
**Regression Risk:** Low to Medium; consumers need a plural/count-compatible contract.  
**Related Findings:** CT-119  

### CT-127 — Archive verification ignores the accepted JSON flag

**Severity:** Low  
**Category:** CLI contract  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Archive verify dispatch and human renderer  
**File Locations:** `crates/ct-cli/src/main.rs:686-695`; `crates/ct-cli/src/render.rs:2611-2661`  
**User Impact:** Automation requesting JSON receives prose that cannot be parsed.  
**Description:** The shared archive command accepts --json but the verify branch never passes it to a renderer.  
**Evidence:** archive_integrity has no JSON argument or serialization branch.  
**Root Cause:** Captured synthetic fixture CLI archive --verify --json exited 0; serde_json rejected stdout as non-JSON. No real archive accessed.  
**Reproduction / Verification:** Pass the flag through and serialize a stable id/integrity DTO; test each integrity outcome and the combined flags.  
**Recommended Solution:** Reliable machine-readable archive checks.  
**Expected Benefit:** undefined  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** None  

### CT-129 — CLI inspection prints untrusted terminal controls from metadata

**Severity:** Low  
**Category:** Security / terminal output  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Codex metadata and CLI inspect headers  
**File Locations:** `crates/ct-adapters/src/codex/parse.rs:492-493,1258-1269`; `crates/ct-cli/src/render.rs:675-686`  
**User Impact:** Explicitly inspecting an attacker-influenced local log can manipulate terminal display and conceal or overwrite visible output.  
**Description:** Decoded cwd/model/git metadata reaches terminal print calls without escaping controls.  
**Evidence:** A synthetic copy of the public fixture changed cwd to a JSON-escaped ESC[2J sequence; captured stdout contained the decoded terminal control.  
**Root Cause:** Metadata headers bypass the sanitizer used for message/raw display.  
**Reproduction / Verification:** Isolated CLI child used only fixture homes and a new archive root; stdout was captured and searched as bytes, never rendered. Exit 0. Terminal-specific clipboard or command execution was not tested or claimed.  
**Recommended Solution:** Apply a common terminal-safe formatter to every untrusted human-output field, including IDs, paths, model/branch and errors; preserve exact values in JSON through normal JSON escaping. Test ESC, C0/C1 controls and multiline fields.  
**Expected Benefit:** Prevents local evidence from controlling CLI terminal presentation.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** CT-110, CT-111  

### CT-130 — An entirely unreadable corpus is reported as empty

**Severity:** Low  
**Category:** CLI error communication  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Corpus aggregation and human stats  
**File Locations:** `crates/ct-cli/src/render.rs:142-155`; `crates/ct-application/src/corpus.rs:192-211`; `crates/ct-application/src/lib.rs:676`  
**User Impact:** Users are told no sessions exist when discovered sessions all failed to load, hiding the need for recovery or permission fixes.  
**Description:** The zero-success branch returns before printing unreadable count.  
**Evidence:** add_unreadable increments only unreadable; successful add increments sessions. stats tests sessions==0 before the warning.  
**Root Cause:** Empty discovery and failed parsing share a presentation branch.  
**Reproduction / Verification:** Deterministic source path with sessions=0 and unreadable=1; JSON carries the correct count. No private or unreadable user file was opened.  
**Recommended Solution:** Distinguish zero discovered sessions from zero successful loads; print excluded counts before returning, with doctor guidance. Add zero-success/one-failure and mixed-success cases.  
**Expected Benefit:** Accurate diagnosis of discovery and parsing failures.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** None  

### CT-132 — Codex framing-budget edge silently hides oversized messages

**Severity:** Low  
**Category:** Parser fidelity  
**Status:** Confirmed  
**Confidence:** High  
**Affected Components:** Bounded JSONL reader and Codex oversized classification  
**File Locations:** `crates/ct-adapters/src/jsonl.rs:106-117,133-169`; `crates/ct-adapters/src/codex/parse.rs:407-454`  
**User Impact:** A narrow record-size boundary can omit a context-bearing message while reporting no unrecognized event.  
**Description:** The reader retains MAX_PARSE_BYTES+2 for framing. A record with content size MAX_PARSE_BYTES+1 or +2 can be oversized yet nontruncated; unsupported oversized kinds fall back to SessionEvent.  
**Evidence:** Valid response_item/message at exactly 4 MiB+1 was classified as metadata with unrecognised_total=0. Larger drained records correctly hit the truncated guard.  
**Root Cause:** Oversized fallback conflates safely ignorable metadata with unsupported context-bearing content.  
**Reproduction / Verification:** Compiled probe created only a synthetic 4 MiB+1 JSON message in ignored target; adapter load confirmed SessionEvent and zero warnings (exit 0). This is not a broad claim about all records above 4 MiB.  
**Recommended Solution:** Use Unrecognised/incomplete classification for oversized shapes not explicitly supported; add MAX−1/MAX/MAX+1/MAX+2/MAX+3 cases with LF, CRLF and EOF framing.  
**Expected Benefit:** No silent fidelity gap at the reader budget boundary.  
**Estimated Effort:** Small  
**Regression Risk:** Low  
**Related Findings:** None  

## Architectural Recommendations

1. **Request-input evidence boundary — immediate, CT-101.** Current reconstructors use response/report anchors for input inventories. Introduce explicit input/completion boundaries and membership confidence before tuning calibration. Benefit: interpretable downstream diagnostics. Tradeoff: legacy formats need honest ambiguity handling. Migrate fixtures and semantic baselines together; balanced totals must not justify wrong membership.

2. **Separate observation from alerts — high, CT-103/119/120/121.** The monitor owns change observation, cache invalidation and delivery. Give a lightweight observer changed/deleted/rotated identities and feed a separately configured rule engine. Reuse domain capacity policy and compound identity. Benefit: reliable live follow and isolated tests. Tradeoff: explicit observer/checkpoint lifecycle. Preserve baselining, coalesce changes and define restart/key-migration behavior.

3. **Bounded work and transactional persistence — high, CT-104/105/109/123.** Synchronous handlers block native dispatch. Add async facades with bounded blocking jobs and operation identity/cancellation. Benefit: responsiveness. Tradeoff: concurrency exposes writer races. Stabilize archive commits/recovery and unique scratch files before enabling overlap; preserve last acknowledged copies during migration.

4. **Common filesystem/redaction contracts — high, CT-110/111/118.** Instruction analysis reads paths outside native Open policy; archive scanning loses JSON context. Inject a bounded local reader and share OS-boundary path/action policy. Detect decoded credentials with member context, replace structurally. Benefit: consistent authority/privacy and isolated testing. Tradeoff: legitimate home files, active associations and byte identity. Migrate with typed refusals and compatibility regressions.

5. **Incremental analysis primitives — medium, CT-123.** Per-turn reconstruction rebuilds session inventories/indexes. Add immutable session indexes and branch-aware aggregates, with release benchmarks and semantic equivalence tests. Benefit: less repeated work. Tradeoff: revision/compaction/branch invalidation; never assume append-only files.

6. **Frontend operation identity and partial availability — medium, CT-106/108/112/114/116.** Many features guard request generations, but optional context blocks browsing and lifecycle variants diverge. Share request/subscription helpers; represent metadata/context/transcript/errors independently. Extract coherent feature state from App.tsx while changing it, without a wholesale rewrite. Strengthen nested contracts and render containment. Benefit: consistent recovery and testability. Tradeoff: preserve focus/selection and deferred-response behavior during incremental migration.

## Performance Findings

### Measured: CT-123 synthetic debug calibration

| Turns | Events | derive_ratio time | Doubling |
|---|---|---|---|
| 250 | 500 | 446.92 ms | Baseline |
| 500 | 1000 | 1675.30 ms | 3.75× |
| 1000 | 2000 | 6627.46 ms | 3.96× |

Programmatic linear Claude histories with short constant-size content; existing debug libraries; one sample/size. Parsing, disk, IPC, React, release optimization, OS cache and RSS were not measured. This establishes scaling under the harness, not a production SLA. Correct CT-101 before rebaselining. The combined harness later failed an unrelated assertion because notification defaults disable evaluation; timing samples completed. Corrected notification probe passed separately.

### Algorithmic concerns; user-visible magnitude unverified

- **P-02 — Transcript seeking/rendering:** App.tsx:3305-3316 sequentially loads pages until a target arrives and retains/renders accumulated rows (3322-3396). transcript.rs:153,205-215 rebuilds visible-event indexes for each page/entry. Late seeking repeats indexing and grows DOM work. Profile target latency/DOM count, then consider indexed direct seeking and windowed rows preserving reveal/filter/focus. No measured jank claim.
- **P-03 — Notification growth/I/O:** adapters/notifications.rs:35-48,79-107,139-149,212-226 retains dedupe/checkpoints beyond visible feed retention. Full-state reads repair keys by vector membership; mutations rewrite JSON, including listing. Monitor checkpoint lookups repeat per descriptor. Benchmark long-lived histories before choosing indexing/retention. Clearing history intentionally preserves dedupe; pruning needs cursor semantics. No measured RSS/disk claim.
- **P-04 — Invalidation:** Arc caches and content-analysis upgrades avoid repeated ordinary turn parsing, but misses/invalidation recalibrate fully. Instrument parse/calibration/content/cache separately. CT-109 independently establishes dispatch blocking.

The desktop_perf example documents cache caveats but discovers the real operator corpus; read, not executed. Follow-up profiling must isolate synthetic homes/archives and use release binaries. No startup/soak/memory-leak measurement is claimed.

## Security Findings

JSONL, attachment labels, tool output and paths are untrusted evidence. Malicious workspaces/tools or imported/modified sessions can influence them. A same-user attacker with arbitrary execution already has broader access; the review focuses on unexpectedly expanded authority.

| Boundary | Safeguards reviewed | Residual concern / prerequisites |
|---|---|---|
| JSONL → domain | Bounded parse/raw reads, malformed evidence, source offsets, ancestry safeguards | CT-101 semantics; universal format compatibility unproven |
| IPC → React | Typed DTOs, runtime checks, escaped text | H-05 nested contract/containment; no confirmed untrusted HTML execution |
| Logged target → OS | Evidence-derived identity, canonical/locality checks, denylist | CT-110 malicious local CPL plus explicit click; no payload executed |
| Attachment → file read | Explicit comparison, surfaced errors | CT-111 remote/device/drive-relative paths and unbounded reads; SMB authentication conditional/unexecuted |
| Session → archive/MCP | Default redaction, explicit raw opt-out, value-free reports | CT-118 contextual JSON credential gap; Redacted does not mean secret-free |
| Session → external CLI | Validated identity, encoded PowerShell data, cwd/CLI checks, preview | Real restoration unverified; no injection established |
| Activation → navigation | Numeric durable IDs, startup queue, XML escaping | Actual Windows toast/deep-link activation untested |
| Catalog/update → runtime | HTTPS, updater signature key, constrained endpoint, Woodpecker gates/digests | Current advisories/assets and clean-host update unverified |

CT-110/111 detail attacker input, boundary, prerequisites, code and impact. CT-118 uses a synthetic value; no real credential was used/exposed. No private corpus was inspected. Ordinary parsing does not upload sessions. Synthetic values were confined to fixture/ignored test artifacts and are not copied here.

Cargo.lock/package-lock and relevant pinned desktop code were inspected. No advisory scan, installed ACL review, Authenticode/clean-host verification or OS exploit was performed; dependency/update safety is not certified.

## UI/UX Findings

| Journey | Source/test assessment | Limits |
|---|---|---|
| First launch/discovery | Roots/warnings and labelled demo; omission transparency deserves further review | No native first launch/permission tree run |
| Projects/filtering | Visibility reversible; CT-114 stale options, CT-122 recency | DOM/source |
| Open/context | CT-106 metrics block browsing; CT-101 input membership | Fixture probe; no visual verification |
| Tokens/cost | Confidence/unpriced/cache provenance useful; CT-102/107/113 errors | Synthetic billing/source |
| Transcript/search | Paging, expansion, retry, highlight; P-02 seeking scaling | No timing/geometry |
| Compare/export | CT-112 recovery/stale results, CT-108 late attribution | Source ordering |
| Resume | Agent-specific preview/copy/validation | Stub only, no real restoration |
| Notifications | CT-103 follow coupling; CT-119/120/121 fidelity | Pure candidates, no native toast |
| Archive/recovery | Verify/fallback and explicit raw choice; CT-104/105/117/118 | Isolated smoke; no fault injection |
| Errors | Many retries/generation guards; CT-106/108/112/116 exceptions | Source/DOM, H-05 containment gap |
| Updating | User-initiated signed updater/status handling | No install/update/restart |

Accessibility review covered modal focus/restoration, inert background, keyboard tabs/sidebar, SVG controls, reduced motion and visibility CSS. No further defensible source defect emerged. Screen-reader output, contrast, focus/scroll geometry, narrow-window overflow, hierarchy and installed WebView remain unverified. Existing screenshots are not current acceptance.

## Testing Gaps

Gaps describe missing behavioral evidence, not absence of all nearby unit tests.

| Priority | Missing regression / harness | Value |
|---|---|---|
| Immediate | Both agents exclude current response, retain prior responses; multi-block/tool/compaction/rewind (CT-101) | Prevents balanced but causally impossible inventories |
| Immediate | Partial manifest→ingest→restart; failed metadata commit preserves prior copy (CT-104/105) | Prevents acknowledged archives losing recoverability/metadata pairing |
| High | Generic JSON members and escaped credentials, positive controls, archive/MCP (CT-118) | Prefix fixtures miss context-dependent detection |
| High | Native delayed I/O leaves controls responsive; bounded overlap (CT-109/123) | Compilation/DOM tests do not prove scheduling |
| High | Missing input/output/cache versus zero; forecasts and marginal savings (CT-102/107/113) | Prevents precise-looking underestimates |
| High | Follow with alerts disabled; switched model; baseline→append; same IDs/different agents (CT-103/119/120/121) | Tests observer/rule/store assumptions together |
| Medium | No-usage transcript available; compare failure controls (CT-106/112) | Exercises actual backend rejection rather than mocked empty success |
| Medium | Deferred A export→B selection; late/mixed listener registration (CT-108/116) | Prevents stale results and subscription leaks |
| Medium | New projects without filter change; resumed old Codex session (CT-114/122) | Catalog/navigation agreement |
| Medium | Active associations and malicious paths using fake readers, never live share/device (CT-110/111) | Refusal before dangerous I/O |
| Low | MCP number/type bounds, actual archive recovery guidance (CT-117/124) | Stable transport/UI contracts |
| Follow-up | Native keyboard/screen-reader, toast, resume, restart/soak, clean-host install/update/uninstall retention | Shipping acceptance beyond source/unit checks |

Further assessment: parser mutation/property tests, rewrite/rename/delete during reads, multi-process archive writers, advisories, release-mode synthetic performance and resources. Do not install tools, release or inspect private sessions just to fill gaps.

## Positive Engineering Observations

- Layered Rust and shared runtime reduce CLI/desktop drift; ports support isolated filesystem/token/pricing testing and compound identity is used correctly in most cache/navigation paths.
- Source offsets, observed/derived/estimated counts, nullable usage, unpriced turns and comparability refusals are strong foundations. Preserve these; balanced totals do not prove membership.
- JSONL drains oversized records with bounded retention and continues after malformed lines; raw fetches bound allocation and reject short reads.
- Producer-specific ancestry/request grouping/compaction/exact recount and version-labelled fixtures make compatibility reviewable without asserting universal support.
- Exports stream per turn, include residuals, name estimator/fidelity/redaction and propagate sink errors. Reports omit matched secret values; previews redact before truncation; smoke tests include positive controls.
- Evidence-derived Open, validated/encoded resume, strict activation IDs, escaped XML, narrow capabilities and signed updates are substantive safeguards.
- Many React generations and focus/keyboard paths are explicit; demo labelled, visibility reversible and missing-source fallback useful.
- Woodpecker exact-commit gates, trusted Windows packaging/digests and separate clean-host acceptance are sound discipline. Local checks were not treated as release evidence.

## Validation Results

- **Final synthetic fidelity/CLI probe:** rustc stdin linked cached debug libraries to target/audit-final-fidelity-evidence.exe. New ignored target/audit-final-fidelity-GUID only; public Codex fixture copied into explicit CODEX_HOME, empty Claude home and isolated archive. Exit 0. Two-node Claude cycle returned 100,000 items (CT-131); growth used 40% instead of recorded 80% (CT-119); two marks counted once (CT-126); captured inspect stdout contained terminal ESC (CT-129, never rendered); archive --verify --json returned prose (CT-127); a valid 4 MiB+1 Codex message silently became metadata (CT-132).
- **npm advisories:** npm.cmd audit --prefix crates/ct-ui --json --ignore-scripts --cache target/audit-npm-cache-GUID. Exit 1 because advisory matches were found: 5 package entries, 3 registry-rated high and 2 moderate; all flagged lock entries dev:true. No installs/fixes or lock edits. See Security Findings for exposure triage, not a shipped-app vulnerability claim.

- **Synthetic archive/family probe:** rustc stdin linked existing debug ct_domain/ct_application/ct_adapters rlibs to target/audit-archive-fault-evidence.exe; ran against a newly verified target/audit-archive-fault-GUID root. Exit 0. Confirmed CT-104 acknowledged-but-unlisted entry, CT-105 failed metadata commit replacing bytes and yielding ArchiveDamaged, and CT-125 cross-agent root collapse. No real archives, permission changes, network or source files.

All results are **local development evidence at the recorded SHA**, not Woodpecker, release or installed acceptance. No dependencies installed. Compiler output went to ignored target, frontend build to ignored dist, smoke homes/archives to a new GUID target directory. No normal application launch, private session discovery, real archive access or workflow dispatch.

| Command / check | Environment | Actual result | Limits |
|---|---|---|---|
| cargo fmt --all -- --check | Windows existing Cargo | Passed, exit 0 | Formatting |
| cargo test --workspace --locked --offline | Cached crates | Passed, exit 0; one ignored manual performance test | Aggregate total not summed; no native UI; resume stub |
| cargo clippy --workspace --all-targets --locked --offline -- -D warnings | Cached crates | Passed, exit 0; 26.99s | Static linting |
| npm.cmd test --prefix crates/ct-ui -- --maxWorkers=1 --testTimeout=15000 | Existing node_modules | 180 tests / 6 files passed, 169.45s | DOM mocks; workers/timeouts differ from CI |
| npm.cmd run build --prefix crates/ct-ui | Existing TypeScript/Vite | Passed; 26 modules; JS392.73kB/gzip114.67kB; CSS100.31kB/gzip18.52kB | Frontend, not native packaging |
| powershell -NoProfile -ExecutionPolicy Bypass -File scripts/release/test-release-notes.ps1 | PS5.1.26100.9444 | Passed | Serialization only |
| powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/check-fixture-manifest.ps1 | Windows PowerShell | 17 valid/catalogued/version-labelled fixtures | Not all formats |
| powershell -NoProfile -ExecutionPolicy Bypass -File scripts/ci/test-fixture-manifest.ps1 | Public fixture temp copies | 4 negative cases passed | Validator tests |
| node --test scripts/release/verify-main-validation.test.mjs | Node24.19.0 | 3 tests passed | Mock statuses, no CI |
| cargo build -p ct-cli --locked --offline | Debug build | Passed, 50.61s | Not release evidence |
| powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/ci/smoke-cli.ps1 -CtExe target/debug/ct.exe | Isolated public fixtures | 2 sessions, inspect/context, 4 dropped items; passed | Debug CLI |
| powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/ci/smoke-archive.ps1 -CtExe target/debug/ct.exe | Separate fixture/archive | 6 redactions; listed fixture values absent; parseable JSON; intact verify | No generic member/fault injection; CT-104/105/118 remain |
| powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/ci/smoke-secrets.ps1 -CtExe target/debug/ct.exe | Separate synthetic fixture | Kinds without values; 4 export-reachable values removed; passed | Known patterns |
| stdin JSON-RPC → target/debug/ct.exe mcp | Isolated fixture homes | 5 responses, no notification response, 17 tools; CT-124 reproduced | No network/pricing/private data |
| stdin Rust → target/audit-evidence.exe | Public fixtures/programmatic billing; existing rlibs | CT-101 both agents, CT-102 missing input/output reproduced; exit0 | No producer/native run |
| stdin Rust → target/audit-redaction-evidence.exe | Synthetic JSON value | CT-118 scanner detects/unchanged archive/0 replacements; exit0 | No value printed |
| stdin Rust → target/audit-notification-evidence.exe | Programmatic sessions, enabled settings | CT-119/120/121 reproduced; exit0 | Pure candidates, no OS/user store |
| stdin Rust → target/audit-notification-performance.exe | Synthetic debug Claude DAG | Three calibration timings above completed | Later harness assertion failed with default disabled notifications; corrected separate probe passed |

Rust1.97.1, Node24.19.0, npm11.17.0 were verified. Cargo executable: C:/Users/anesk/.cargo/bin/cargo.exe. Smoke invocation set only process-local RUNNER_TEMP to a verified new target/audit-smoke-GUID; existing helpers isolated both agent homes and CONTEXTTRACE_ARCHIVE. No operator homes/archive were targeted. Probe source came over stdin, with no additional source files.

Environment/harness errors: restricted shell failed CreateProcessAsUserW access denied and Node child creation failed EPERM; approved shell succeeded. Initial notification probe had default disabled settings and failed expected-candidate assertion; corrected enabled probe passed. An intermediate harness u64/u32 compile mismatch was corrected. These are harness/configuration iterations, not workspace-suite failures. No historical CI outcome is claimed as current evidence.

## Unverified Hypotheses

- **H-01 — Needs Verification:** Archive PID-only scratch names (archive.rs:225-242) permit overlap concerns; no store-wide coordination. Normal simultaneous desktop archive callers have not been demonstrated because IPC is synchronous. Prove a caller/overlap with isolated input before a new concurrency finding. CT-104/105 do not rely on this hypothesis.
- **H-02 — Confirmed CT-109:** Pinned Windows dispatch is inline; actual native freeze duration unmeasured.
- **H-03 — Confirmed CT-123:** Algorithmic work has debug synthetic scaling evidence; release/native/RSS magnitude unknown.
- **H-04 — Confirmed boundaries CT-110/111:** Active file acceptance and unrestricted instruction paths established; execution/authentication intentionally untested, no compromise claim.
- **H-05 — Needs Verification / hardening:** api.ts:179-209 corpus and 1034-1057 cost validators are shallow and omit rendered fields; no render boundary in main/App. Accepted malformed payloads could throw (toolCalls.toLocaleString/average.reduce), but current Rust DTOs are typed and no ordinary-input producer path was established. Contract/containment gap, not a confirmed production crash.
- **H-06 — Needs Verification:** Later poll health changes are not automatically refreshed in settings. Settings themselves synchronously update status.running (notifications.rs:490), disproving an inherent toggle-delay claim. Runtime stale-health impact unmeasured.
- **P-02/P-03:** Transcript and durable notification scaling need release/long-lived measurements before user-visible severity.

Scope corrections retained: Doctor scan is session-wide (CT-115 label wrong); archive reading exists (CT-117 prose wrong); ordinary Codex appends change size and are detected despite CT-122; enabled settings are required for notification probes. No disproved hypothesis was silently promoted.

## Recommended Remediation Roadmap

Recommendations only; no permission to implement.

1. **Immediate safety/correctness:** CT-101 semantic request fixtures; CT-104/105 archive recovery/last-good pairing; CT-118 contextual redaction. Address CT-110/111 OS boundaries using fake-reader/refusal tests.
2. **Reliability:** Offload native work after writer coordination (CT-109); separate observer and fix notification policy/cursors/identity (CT-103/119/120/121); browse unmeasured sessions and preserve unknown costs (CT-106/102/113).
3. **Performance:** Correct semantics before optimizing CT-123; release-mode synthetic budgets; profile transcript/direct seek/windowing and long-lived state (P-02/P-03).
4. **Architecture:** Shared boundaries, observer/jobs and operation identity as above; domain policy across all interfaces; incremental changes, no unrelated rewrite.
5. **UX:** Marginal savings, late export, comparison recovery, project/recency refresh, diagnostic scope, subscriptions, archive guidance and MCP range checks (CT-107/108/112/114/115/116/117/122/124). Verify focus/keyboard/visual behavior after changes.
6. **Longer term:** Format mutation/fault injection, nested contracts/render containment, metadata retention, advisories, restart/soak and separate clean-host installer/updater/resume/toast/uninstall. Packaging/publication stays Woodpecker-only under a separate release task.

## Review Progress Log

- 2026-10-09T00:06:12.280Z — Completed additional CLI and parser/fixture source coverage with follow-up reviewers. Added CT-127..133 after independent code checks; runtime probes strengthened CT-119/126/127/129/131/132. Narrowed oversized finding to the exact framing-budget edge. npm advisory inventory queried without fixes; dependency paths are development-only. No production files changed.

- 2026-10-09T00:02:20.616Z — Saved initial report in commit 802e7e7 (FEEDBACK.md only); application source remains at reviewed SHA. Archive fault probe confirmed CT-104/105 at runtime. Independently verified specialist family/growth paths, added CT-125/126, extended CT-119 to growth/corpus. Frontend follow-up verified updater/replay/preference safeguards and revalidated existing findings; malformed nested DTO remains a hardening hypothesis.

- 2026-10-09T00:00:12.095Z — Continuation verified unchanged application SHA and clean production worktree; FEEDBACK.md was the only untracked file. Two follow-up source reviewers are active, and a CLI reviewer was assigned. User explicitly authorized frequent commits of the audit document, superseding the earlier no-commit rule. No production changes or release operations authorized.

- 2026-10-08T19:50:11.199Z: Consolidated 24 findings (0 Critical, 3 High, 18 Medium, 3 Low), all required sections and next steps. Debug CLI build, three isolated smoke scripts and MCP protocol/boundary probe passed. Final document verification detected stale summary placeholders from a report-helper scope error; rewritten with explicit document arguments and disk verification. Git confirms unchanged SHA and only untracked FEEDBACK.md.

- 2026-10-08T19:45:38.858Z: Offline debug CLI build and all three isolated public-fixture CLI/archive/secret smoke scripts passed. MCP protocol fixture subprocess confirmed CT-124 numeric wraparound; 17 tools listed and notification correctly received no response. Findings now total 24: 3 High, 18 Medium, 3 Low. No production files changed.

- 2026-10-08T19:40:08.015Z: Confirmed CT-123 from aggregate calibration source and synthetic debug scaling measurements. Reviewed full runtime composition, directory walker, bounded raw source, startup/monitor lifecycle and CLI fixture smoke isolation. Building CLI offline to execute existing isolated smoke scripts.

- 2026-10-08T19:38:13.987Z: Independently verified CT-118 through CT-122; synthetic redaction and enabled-notification probes reproduced CT-118/119/120/121. A first notification probe used default disabled settings and failed its expected-candidate assertion; corrected configuration passed. Claude debug calibration benchmark measured 446.92/1675.30/6627.46ms at 250/500/1000 turns. Strict Clippy and four fixture-validator negative cases passed. Consolidation and remaining-coverage inventory underway.

- 2026-10-08T19:32:08.874Z: Completed major frontend and desktop/security source reviews; independently confirmed CT-109 through CT-117. Workspace tests, 180 frontend tests and frontend production build passed. Fixture-only/synthetic Rust probe reproduced CT-101/102. Real desktop, active-file, SMB and release acceptance remain unexecuted.

- 2026-10-08T19:28:35.243Z: Independently checked reconstruction/calibration, incomplete billing, live follow, missing-usage opening, archive crash handling and export/sandbox state. Added CT-102 through CT-108; CT-104 is High and remaining additions Medium. Tracked concurrent archive and native thread dispatch as hypotheses pending stronger verification.

- 2026-10-08T19:23:22.110Z: Read AGENTS.md and CLAUDE.md completely, README/HANDOFF/manifests/release procedure and repository inventory. FEEDBACK.md did not exist and was created before substantive review. Branch/revision identified through approved read-only shell path. Initial restricted shell and Node child processes failed with access denied/EPERM; escalated shell succeeded. Global memory access was rejected by automatic review and abandoned; report relies on repository evidence.

## Remaining Audit Work

- [x] Instructions, previous report state, manifests, architecture/compatibility/release docs and dynamic module/CI inventory.
- [x] Parallel Rust/data, frontend and desktop/security source reviews, coordinator evidence verification/deduplication.
- [x] Selected end-to-end data/cost/archive/observer/filesystem/resume/activation/update flows.
- [x] Offline workspace/frontend checks, fixture/release helper tests, isolated CLI/archive/secret smoke and MCP checks.
- [x] Synthetic input/billing/redaction/notification reproductions and debug calibration scaling.
- [ ] Exhaustive Codex exact/compaction/streams and Claude parser variants against fixture assertions; rewinds, missing IDs/timestamps, truncation and rewrites. Major paths reviewed, not every branch.
- [ ] Complete application diff/lifecycle/ghost/growth/family/corpus and domain pricing edge review beyond selected entry points/consumers.
- [ ] Complete CLI flags/human render/errors and malformed MCP type/bounds/EOF behavior beyond the recorded probe.
- [ ] Every nested frontend contract/render/error path and test assertion; discovery omission transparency and stale-health hypotheses.
- [ ] Isolated fault injection for CT-104/105 and demonstrated concurrent H-01 callers; never real archives.
- [ ] Release-mode synthetic calibration/transcript/startup/cache/RSS and prolonged metadata/resource measurements; never default-home desktop_perf.
- [ ] Dependency advisory inventory with approved existing tools, no installs/lock edits; permissions review only in a non-sensitive setup.
- [ ] Isolated synthetic native visual/screen-reader, responsiveness, activation, shutdown/restart and real saved-context resume. Native automation unavailable here; normal launch risks user-data access.
- [ ] Separate clean-host installer/updater/uninstall retention/signing acceptance. No packaging/release/deployment authorized. Current Woodpecker/assets not refreshed.

Unchecked work prevents exhaustive-completion or production sign-off.

## Resume Instructions

1. Read AGENTS.md, CLAUDE.md, docs/RELEASING.md and this report. Compare branch/SHA/status with main at 8a72cc76a8f1aec04911dc31d48cc63cb3e34d8c. Revalidate affected findings if source changed; preserve contributors’ work. Only edit FEEDBACK.md; no fixes/installs/releases. User subsequently authorized frequent audit-document commits; stage only FEEDBACK.md and do not push without authorization.
2. Continue unchecked source coverage: adapters/codex/{exact,compaction,parse}.rs and streams.rs; application/{diff,lifecycle,ghost,growth,family,corpus}.rs; domain pricing; CLI render/flags/MCP. Read callers/error paths/fixtures together; avoid repeating unchanged fully reviewed runtime.
3. Strengthen CT-104/105 with isolated fault injection and H-01 with an applicable overlap. Synthetic bytes and new ignored target/temp only. Use with_home or process-local fixture homes; never runtime defaults/private archives.
4. Probe executables are ignored artifacts, not committed regression tests. Findings/validation sections and conversation log record construction/results; rebuild stdin harnesses if needed and never assume binaries match a newer SHA. Repeat checks only for new changes/failures.
5. Performance: release synthetic linear/branch/compacted input, repeated samples and memory telemetry; retain debug timings as historical evidence. Native acceptance requires explicitly isolated profiles and supported tooling; jsdom is not visual verification.
6. Update evidence immediately, preserve CT-101..CT-133, allocate CT-134 next, retain resolved/invalidated history, distinguish source/test/native/CI/release. End with counts/limitations/checkpoint. No confirmation needed between normal authorized review stages.
