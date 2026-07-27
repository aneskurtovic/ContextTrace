# BACKLOG.md — the authoritative work queue

**This file decides what gets built next.** `IDEAS.md` is an idea pool, not a
plan: nothing in it is scheduled until it is pulled in here and given an ID.
Work proceeds top-down through `next`, then `todo`, in the order written.

## Entry format

Every entry is a level-3 heading carrying an ID, followed by one metadata line
and two required fields. The metadata line is machine-greppable on purpose:
grepping the status field is how you find the current work without reading the
whole file.

```
### CT-000 · Short imperative title
`status: todo` · `tier: A` · `size: M` · `source: IDEAS.md §2`

**Why:** the reason this is worth doing, in the project's own terms.
**Done when:** the observable condition that closes it. Not "implement X".
```

| Field | Values |
|---|---|
| `status` | `done` · `next` · `todo` · `blocked` · `dropped` |
| `tier` | `A` (cheap, high value now) · `B` (real work, clear value) · `C` (later, or needs the desktop UI) |
| `size` | `S` (hours) · `M` (a day) · `L` (multi-day) |
| `source` | where it came from: `plan`, `IDEAS.md §n`, `review`, `corpus` |

An item pulled from `IDEAS.md` gets a `[→ CT-nnn]` marker added there, so the
idea pool always says what has been scheduled and what has not.

`blocked` and `dropped` entries keep a **Reason** field. Dropped items are kept
rather than deleted — a decision not to build something is worth as much as the
decision to build it, and re-litigating it later is waste.

---

## Done

### CT-001 · Scaffold the Cargo workspace under ports-and-adapters
`status: done` · `tier: A` · `size: M` · `source: plan`

**Why:** the dependency rule is worth nothing if it is a convention rather than
a compiler error.
**Done when:** `ct-domain` depends on no inner crate and `ct-cli` is the only
crate that names a concrete adapter. ✔

### CT-002 · Domain model, ports and calibration service
`status: done` · `tier: A` · `size: L` · `source: plan`

**Why:** confidence has to be un-launderable by construction, not by review.
**Done when:** `TokenCount` variants force presentation code to match before it
can extract a number, and `ContextSnapshot::assemble` rejects a breakdown that
does not sum to its total. ✔

### CT-003 · `AgentAdapter` trait and session discovery
`status: done` · `tier: A` · `size: M` · `source: plan`
**Why:** adding an agent must not touch the domain, application or UI.
**Done when:** discovery honours `CODEX_HOME`/`CLAUDE_CONFIG_DIR` and `ct roots`
prints every path that will be read. ✔

### CT-004 · Codex adapter (replay reconstruction)
`status: done` · `tier: A` · `size: M` · `source: plan`
**Why:** Codex logs the literal API item list, so membership is observed.
**Done when:** replaying `response_item` lines reproduces the request body and
`base_instructions` is accounted for as context. ✔

### CT-005 · Claude Code adapter (parentUuid DAG walk)
`status: done` · `tier: A` · `size: L` · `source: plan`
**Why:** reading the file in line order includes abandoned rewind branches, so
it produces a breakdown that looks plausible and is wrong.
**Done when:** reconstruction walks ancestors, stops at compaction boundaries,
and groups assistant lines by `requestId`. ✔

### CT-006 · Token accounting and calibration
`status: done` · `tier: A` · `size: M` · `source: plan`
**Why:** Codex *could* be counted exactly and Claude Code cannot, so the
asymmetry has to survive into the output. (In the event neither is: see CT-035.)
**Done when:** estimates are scaled to the observed total and the unattributed
remainder is an explicit row, not smeared across categories. ✔

### CT-007 · `ContextSnapshot` reconstruction engine
`status: done` · `tier: A` · `size: M` · `source: plan`
**Done when:** both reconstruction strategies produce a balanced snapshot. ✔

### CT-008 · CLI: roots / sessions / inspect / context / largest / doctor
`status: done` · `tier: A` · `size: M` · `source: plan`
**Why:** the milestone gate — prove the core before any UI work.
**Done when:** `ct sessions` → `ct inspect` → `ct context --turn N` yields a
useful, correctly-totalled breakdown for both agents. ✔

### CT-009 · Standalone JSONL fixtures for both adapters
`status: done` · `tier: A` · `size: S` · `source: plan`
**Why:** real logs can never be committed, so the regression surface has to be
synthetic and hand-authored.
**Done when:** each fixture encodes one way the real format misleads a reader,
including an unknown event type asserting graceful degradation. ✔

### CT-010 · Verify against the real local corpus
`status: done` · `tier: A` · `size: S` · `source: plan`
**Done when:** the largest sessions parse with zero failures and 100% fidelity,
totals match raw JSONL, and an mtime check proves nothing was written.
✔ 150/150 (126 Claude Code, 24 Codex), zero unrecognised, read-only confirmed.

### CT-011 · Read the prompt size from one API call, not a sum across calls
`status: done` · `tier: A` · `size: M` · `source: corpus`

**Why:** Claude Code's top-level `cache_creation`/`cache_read` are sums across
an `iterations` array (466 of 474 records), so the reported prompt could exceed
any context window — 844,611 tokens for a turn whose largest call was 429,328.
This corrupted `TokenCount::Observed`, the most trusted figure in the tool.
**Done when:** the largest single call is used and `ct doctor` names the
affected turns. ✔

### CT-012 · Count model-visible text, not serialized JSON
`status: done` · `tier: A` · `size: S` · `source: corpus`
**Why:** serialising a block counts key names, braces and the escaping that
doubles every newline and Windows path separator — and it distorts unevenly, so
it does not cancel out in calibration.
**Done when:** text leaves are counted, `tool_use.input` still counts as JSON,
and images are capped at their documented ~1,600-token ceiling. ✔

### CT-013 · Size redacted thinking from its signature
`status: done` · `tier: A` · `size: S` · `source: corpus`
**Why:** 99.2% of thinking blocks log no text and only a signature, and that
reasoning still occupied context.
**Done when:** size is derived by regression (slope 2.353, R² 0.97 — not the
size-biased median ratio) and flagged as derived in `ct doctor`. ✔

### CT-014 · Derive characters-per-token per session
`status: done` · `tier: A` · `size: L` · `source: corpus`
**Why:** the true ratio spans 1.49–3.49 across the corpus. A uniform error
cancels out of the proportions but **not** out of the residual — and the
residual is the unlogged system prompt and tool schemas, the thing the tool
exists to name.
**Done when:** the ratio is fitted from consecutive-turn deltas, the constant is
recovered afterwards from the levels, and sessions report a measured unlogged
figure where they previously could not. ✔

### CT-015 · Keep subagent transcripts out of the main thread
`status: done` · `tier: A` · `size: S` · `source: review`
**Why:** the domain documented sidechains as "must never be folded into" the
main context and nothing enforced it. No session in the corpus uses subagents,
so no real-data check could ever have caught it.
**Done when:** the ancestor walk keeps only events on the same side as the turn
being asked about, in both directions, with fixture coverage. ✔

---

## Next

### CT-018 · `ct trace` — follow one context item's lifecycle
`status: next` · `tier: A` · `size: M` · `source: IDEAS.md §2`
**Why:** answers "when did this enter context, and when did it leave" — the
question `ct context` cannot, because it only sees one turn.
**Done when:** `ct trace <id> --item <ref>` reports the turn an item entered,
the turns it persisted through, and the compaction that evicted it.

---

## Done (continued)

### CT-034 · Name a tool output by its target, not just its tool
`status: done` · `tier: A` · `size: M` · `source: review`

**Why:** CT-017 made the 38k-token tool result findable and then showed it as
`Tool output: Read` — twice, at 14,805 and 7,822 tokens, with nothing to say
which file each one was. The filter answers "where did this come from" at the
level of the *mechanism*; the workflow needs it at the level of the *thing*.
**Done when:** a tool call and its result are labelled with what they acted on —
the path for a read, the command for a shell call — for both agents, taken from
the call's own arguments and never invented where they are absent. ✔

**The design decision worth keeping.** Encoding "Read takes `file_path`, Bash
takes `command`, Grep takes `pattern`" would mean editing a file every time
either agent ships a tool, and degrading silently for MCP tools nobody here has
heard of. Instead an ordered list of argument names is tried, most specific
first, and the first present wins — so an unknown tool taking a `path` or a
`query` is named correctly without anything knowing it exists. Where no key
matches, the label stays the bare tool name: a wrong filename is worse than no
filename.

A second defect surfaced only once real paths were in the rows. Truncating
`C:\Users\anesk\source\repos\VoxMux\BACKLOG.md` from the right yields a label
naming a machine and a repository but not a file — the one thing being looked
for. Long labels now lose their middle instead, which suits paths (informative
tail) and commands (informative head) alike.

### CT-017 · Filter context views by provenance and size
`status: done` · `tier: A` · `size: S` · `source: IDEAS.md §2`
**Why:** named in the brief as essential to the core workflow — finding the
38k-token garbage tool result.
**Done when:** `ct context`/`ct largest` accept `--source`, `--category`,
`--confidence` and `--min-tokens`, and the filtered rows still state what share
of the whole they represent. ✔

**The design decision worth keeping.** Filtering creates one obvious way to
lie: recompute the percentages against the subset, so four tool outputs
"account for 100% of the context". `FilteredView` borrows the snapshot rather
than owning the matched items, so `total()` is only reachable through the
aggregate and there is no subset sum to divide by. The unfiltered views are the
`ItemFilter::ALL` case of the same code path, so the two cannot disagree about
the denominator. The residual is excluded from any filtered view unless named
by category: it has no source and no line in any file, so a query *by
provenance* has nothing to match it against.

`--confidence` matches nothing on every real session today, because per-item
sizes are `Estimated` for both agents. That is not a broken flag — see CT-035 —
so the empty case prints the categories, sources and confidences the turn
actually contains rather than an unexplained blank.

### CT-016 · `ct residual` — track unlogged context across turns
`status: done` · `tier: A` · `size: M` · `source: IDEAS.md §2`

**Why:** the residual only became a measurement in CT-014, and it is stable by
nature — the system prompt and tool schemas do not change mid-session. So a step
change in it means the harness altered them: a tool was registered, an MCP
server connected, a skill loaded. That is a context change no other view can
show, because nothing in the log records it.
**Done when:** `ct residual <id> [--from N] [--to N] [--json]` prints per-turn
prompt size, accounted tokens and unlogged remainder, flags step changes, and
prints `not measurable` for turns where reconstruction over-counts rather than
showing a misleading zero. ✔

**What building it taught us — worth recording, because it contradicts the
idea's premise.** The remainder is *not* the constant the idea assumed. It
drifts upward across a session, because one fitted ratio cannot describe a
content mix that starts as prose and ends dominated by tool output, and whatever
the ratio gets wrong lands here. Differencing adjacent turns therefore flagged
roughly 40 of 104 turns on the first real session — no signal at all.

What separates a real change from drift is **persistence**: a registered tool
stays registered, while fit wobble reverts within a turn or two. Comparing the
median of the five turns either side cut that session from ~40 flags to 6.
Steps adjacent to a compaction are attributed to it rather than to an
unrecorded harness change, and the direction is not over-read: a rise means
hidden content was added, a fall means reconstruction began accounting for more,
which this view cannot distinguish from the CT-031 over-count.

---

## Todo

### CT-035 · Offer exact Codex counting, or state plainly that it is absent
`status: todo` · `tier: A` · `size: S` · `source: review`

**The docs half landed with CT-017**; what remains is the opt-in.

**Reason it exists:** the plan, the `TokenEstimator` port docs and CT-006 all
said Codex items are counted exactly by `tiktoken`. They are not.
`TiktokenEstimator::count_text` — the only exact path — is never called by
either adapter. Both size items from the `char_len` recorded at parse time and
so go through `estimate_from_chars`, which returns `Estimated` by construction.
The *reason* is sound and deliberate: exact counting means re-reading and
re-parsing every line of a 55 MB session, which is what `SourceRef` and the
lazy-content design exist to avoid. The claim is what is wrong, not the code.
**Done when:** the docs describe the trade-off actually made, and either exact
counting is offered as an opt-in for Codex or its absence is stated plainly.
Leaving a stronger claim in the docs than the code delivers is the one failure
mode this project cannot afford. ⟨README, `ports.rs` and the adapter table now
say what the code does; the opt-in is what is left.⟩

### CT-019 · `ct doctor --dir` — format-drift reporting across many sessions
`status: todo` · `tier: A` · `size: S` · `source: IDEAS.md §2`
**Why:** the corpus sweep already found five new event types this way; it should
be a command rather than a scratch script.
**Done when:** a histogram of unrecognised types across a directory, with a
non-zero exit code when recognition drops, so it can gate CI.

### CT-020 · `--format ndjson` export
`status: todo` · `tier: B` · `size: S` · `source: IDEAS.md §2`
**Why:** gets most of the analytical value of a database export while keeping
the dependency tree auditable. See CT-032, which it replaces.
**Done when:** turns and context items stream as NDJSON with stable tagged
types.

### CT-021 · `ct diff A..B` — compare two sessions or turn ranges
`status: todo` · `tier: B` · `size: M` · `source: plan`
**Why:** "it worked yesterday and fails today on the same task."
**Done when:** structural differences in composition, tool usage and residual
growth are reported side by side.

### CT-022 · Context growth timeline
`status: todo` · `tier: B` · `size: S` · `source: plan`
**Done when:** per-turn prompt size renders as a sparkline with compaction
boundaries marked.

### CT-023 · Duplicate context detection
`status: todo` · `tier: B` · `size: M` · `source: IDEAS.md §4`
**Why:** agent retry loops re-inject identical file content, and it is invisible
in a per-turn view.
**Done when:** identical content appearing more than once in a turn's context is
reported with its total cost.

### CT-024 · Waste and low-entropy detection
`status: todo` · `tier: B` · `size: M` · `source: IDEAS.md §4`
**Done when:** large low-information blocks are ranked by compressed-size ratio.

### CT-025 · Secret scanning and redacted export
`status: todo` · `tier: B` · `size: M` · `source: brief`
**Why:** the brief requires optional secret redaction for exports, and this is
the one item where being wrong has consequences outside the tool.
**Done when:** exports can be redacted, and scanning never writes findings
anywhere outside the user's terminal.

### CT-026 · Cost projection
`status: todo` · `tier: B` · `size: S` · `source: IDEAS.md §4`
**Done when:** per-category cost is derived from a local pricing table, clearly
marked as an estimate that depends on a table which will go stale.

### CT-027 · Codex compaction diff engine
`status: todo` · `tier: B` · `size: M` · `source: IDEAS.md §1`
**Why:** Codex records `replacement_history` verbatim, so what was discarded is
*derivable rather than inferable* — a stronger claim than anything available for
Claude Code.
**Done when:** the exact items dropped by a compaction are listed with sizes.

### CT-028 · Session family trees
`status: todo` · `tier: C` · `size: M` · `source: IDEAS.md §9`
**Why:** the DAG already parsed for reconstruction contains every abandoned
branch; showing them is nearly free and no other tool does it.
**Done when:** branches are enumerated and each is separately inspectable.

### CT-029 · `ct-index` SQLite cache
`status: todo` · `tier: C` · `size: L` · `source: plan`
**Why:** deliberately deferred until a command feels slow, so the access
patterns are known before the cache is tuned. `ct context` on the largest
session now takes ~4s because CT-014 reconstructs every turn — this is the first
real evidence for it.
**Done when:** derived metadata is cached, disposable and rebuildable, and no
domain type depends on it.

### CT-030 · Tauri v2 desktop shell
`status: todo` · `tier: C` · `size: L` · `source: plan`
**Why:** milestone 3. Blocked on toolchain, not design.
**Done when:** the desktop app calls the same crates through `#[tauri::command]`
with no logic duplicated. Requires switching to the MSVC toolchain first.

### CT-031 · Investigate reconstruction over-count
`status: todo` · `tier: B` · `size: M` · `source: corpus`
**Why:** roughly one Claude Code session in seven accounts for more content than
its prompt held, so its unlogged remainder cannot be measured. The leading
hypothesis is that the harness drops old context without recording it — linear
chains, no rewinds, several times more content than the reported prompt — but
no marker for such a removal exists anywhere in the log.
**Done when:** either a mechanism is identified from evidence, or the hypothesis
is written up as unresolvable from logs alone and the tool's reporting of it is
final. Inventing semantics for it is explicitly out of scope.

---

## Blocked / dropped

### CT-032 · DuckDB export
`status: dropped` · `tier: B` · `size: M` · `source: IDEAS.md §2`
**Reason:** conflicts with the minimal-dependency posture that makes "nothing
leaves this machine" auditable rather than merely promised. CT-020 delivers most
of the value for none of the cost. Revisit only if NDJSON proves insufficient in
practice.

### CT-033 · Replay-to-Prompt
`status: dropped` · `tier: C` · `size: M` · `source: IDEAS.md §8`
**Reason:** requires an HTTP client in the dependency tree, which would break
the structural form of the local-first guarantee — the property that no
network-capable crate is present at all. That guarantee is worth more than the
feature.
