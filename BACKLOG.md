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

## Next

### CT-021 · `ct diff A..B` — compare two sessions or turn ranges
`status: next` · `tier: B` · `size: M` · `source: plan`
**Why:** "it worked yesterday and fails today on the same task."
**Done when:** structural differences in composition, tool usage and residual
growth are reported side by side.

---

## Todo

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

**CT-035 supplied the instrument this needs.** Until `--exact`, an over-count
could always have been the character ratio running high, so there was nothing to
investigate that was not first an estimator question. With real tokenizer counts
the two separate: on Codex the exact sum exceeds the observed total on 1 of 53
local sessions, and that surplus cannot be estimation error. Start there — it is
a small, exactly-measured case of the same defect.

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

**The subset mattered.** CT-019 later swept all 774 rather than the largest 150
and found 99.60% — three types this build does not parse. Sampling the largest
sessions is a good proxy for parse *robustness* and a poor one for format
*coverage*, because a new event type appears wherever the feature was used, not
wherever the file is big.

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

### CT-018 · `ct trace` — follow one context item's lifecycle
`status: done` · `tier: A` · `size: M` · `source: IDEAS.md §2`
**Why:** answers "when did this enter context, and when did it leave" — the
question `ct context` cannot, because it only sees one turn.
**Done when:** `ct trace <id> --item <ref>` reports the turn an item entered,
the turns it persisted through, and the compaction that evicted it. ✔

**The design decision worth keeping.** Presence is swept by reconstructing every
turn, not inferred from `first_seen_turn`. Those answer different questions —
one is when a line was written, the other is when it entered a request — and
where they disagree the view prints both rather than reconciling them. The sweep
runs on the character probe because membership does not depend on the estimator;
size comes from one properly calibrated snapshot, computed exactly as the ranked
view computes it. The two agree *at a given turn* and routinely pick different
ones — `ct largest` defaults to the session's peak, this sizes at the last turn
holding the item — so the same item reads as 12.3% of 73,138 there and 2.6% of
339,687 here. Same token count, different denominator, and both lines name the
turn they used.

**Three refusals encoded as a sum type, not as prose.** A departure is a
`Compaction`, a `BranchDiverged` or an `Unexplained`, because they are not
degrees of one claim: one is read from the log, one is inferred from the shape of
the DAG, and one is an admission. *Gone is not evicted* — when a Claude Code item
disappears with no compaction the later turns descend from another branch, and
the item was never in their prompts to be evicted from (47 of 86 recent local
sessions contain such a fork). *Absent is not unknown* — an unreadable turn ends
a run rather than being read across or blamed for a departure. *A subagent's
turns are not this thread's turns* — its own context window means main-thread
items are legitimately missing there, and counting that as absence would make
every long-lived item flicker.

For Codex the replay fold only clears at a compaction, so `Unexplained` is
unreachable unless ContextTrace itself is wrong — the view says exactly that
instead of inventing a branch a linear log cannot have. A sweep of 240 items
across 40 local sessions produced 192 still-present, 48 removed by compaction,
and no unexplained case.

The item id now appears in `ct largest`, because it is this command's argument
and the label alone is often a long path and never unique.

### CT-036 · Keep the Codex system prompt across a compaction
`status: done` · `tier: A` · `size: S` · `source: CT-018`

**Why:** found by the first real `ct trace` run, which reported *"Codex system
prompt — left after turn 79, the compaction at turn 80 removed it"*. It did not:
`base_instructions` is not part of the item list a compaction replaces. Codex
sends it as the request's own instructions field, and across every compaction in
the local corpus — 47 events in 15 sessions, 593 `replacement_history` entries,
450 user messages, 96 developer messages, 47 opaque `compaction` blobs — no
entry carries the system role.
**Done when:** reconstruction retains the system-prompt item across a compaction
and drops the conversation, with a fixture asserting both halves. ✔

The blast radius was wider than one label: every post-compaction Codex turn was
understating its accounted content and inflating its unattributed remainder by
the size of the system prompt — 4,336 tokens on the session that surfaced it.
A view built to answer "when did this leave" is a good detector for
reconstruction wrongly dropping things.

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

`--confidence derived` matched nothing on any real session when this landed,
because per-item sizes were `Estimated` for both agents. That was not a broken
flag but a missing one, and CT-035 supplied it: `--exact` promotes the
countable Codex items to `Derived`, so the filter now selects something. The
empty case still prints the categories, sources and confidences the turn
actually contains rather than an unexplained blank, because on Claude Code it
remains permanently empty by construction.

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

### CT-035 · Offer exact Codex counting, or state plainly that it is absent
`status: done` · `tier: A` · `size: S` · `source: review`

**Reason it existed:** the plan, the `TokenEstimator` port docs and CT-006 all
said Codex items are counted exactly by `tiktoken`. They were not.
`TiktokenEstimator::count_text` — the only exact path — was never called by
either adapter. Both sized items from the `char_len` recorded at parse time and
so went through `estimate_from_chars`, which returns `Estimated` by
construction. The claim was what was wrong, not the code.
**Done when:** the docs describe the trade-off actually made, and either exact
counting is offered as an opt-in for Codex or its absence is stated plainly. ✔
Both: `ct context --exact` and `ct largest --exact`, Codex only, with Claude
Code refusing through `PortError::Unsupported` rather than a docs footnote.

**Three things measuring it changed.**

*The stated cost was wrong by two orders of magnitude.* This entry said exact
counting "means re-reading and re-parsing every line of a 55 MB session". It
does not: `SourceRef` seeks to a byte offset, so it is one seek per item. On
the largest local Codex session — 99 MB, 262 items at the peak turn —
`ct context` takes 0.43 s and `--exact` takes 0.50 s. The deferral was
justified by a cost nobody had measured.

*It covers about three items in five, and that had to become output rather
than a caveat.* Of 20,768 `response_item` lines in the local corpus, 6,091
carry `encrypted_content`, 2,498 a structured `output` object, 99 an inline
`image_url`. Tokenizing any of those produces a wrong number wearing an
`Exact` label: a ciphertext blob is not the text the model read, image data is
charged as patches, and a structured output would be measured in `serde_json`'s
key order rather than Codex's. `content_text` returns `None` for them — a
refusal, not a failure — and the view prints how many items were actually
measured, because "exact" is a claim about the numbers under it.

*Calibration needed no change, which is the strongest evidence the rule was
right.* Its first rule was already that measured counts are never rescaled, so
exact items keep their values and the slack becomes residual. The one branch
that does rescale them — measurements exceeding the observed total, i.e. the
reconstruction has been disproved — would otherwise have printed calibrated
figures under a flag named `--exact`. It fires on 1 of 53 local Codex sessions,
and now says so in words. That is CT-031 becoming measurable: with exact
counts, over-count is no longer confounded with estimator error.

**And one thing it deliberately does not buy.** The obvious conclusion — that
with every item measured the remainder becomes purely unlogged context — is
wrong, and asserting it was the first thing review caught. An exact count is an
item's model-visible text; the field names, role markers and block structure
around it are excluded on purpose, because counting serialized JSON is CT-012's
mistake. On two fully-exact Codex turns the remainder came to **10,218 and
9,630 tokens** (53% and 42% of the prompt) with nothing estimated at all. That
is tool schemas plus request framing, and `ct context --exact` now says so
rather than reusing the "plus whatever the estimates missed" clause, which is
false where nothing was estimated. Those two figures are also the first clean
measurement of Codex's schema overhead, which no other view can reach.

### CT-019 · `ct doctor --dir` — format-drift reporting across many sessions
`status: done` · `tier: A` · `size: S` · `source: IDEAS.md §2`
**Why:** the corpus sweep already found five new event types this way; it should
be a command rather than a scratch script.
**Done when:** a histogram of unrecognised types across a directory, with a
non-zero exit code when recognition drops, so it can gate CI. ✔

**The gate is on presence, not on a percentage.** A fidelity threshold would
hide the case worth catching: a new event type appearing once in half a million
events is the same news as one appearing everywhere — an agent shipped
something this build does not parse. So `is_clean()` is `types.is_empty() &&
unreadable.is_empty()`, and unreadable files are counted apart from unrecognised
types because a truncated write is not a format change.

**Sessions-per-type is the figure that makes the histogram readable.** A raw
count cannot separate a long-running experiment in one session from a change
that has shipped to all of them, so every row carries `n/total` and one example
id to run `ct inspect` against.

**`--dir` narrows the discovered sessions rather than walking a directory.**
The agent behind each file is then known from its descriptor and nothing has to
be sniffed from contents. Detecting an agent from a file's first line would mean
inventing a rule, and a misdetected file reports as wholesale drift — the
loudest possible way for a guess to be wrong.

**A mistyped `--dir` fails rather than passing.** Sweeping nothing and finding
no drift is arithmetically clean and semantically useless — it would be a green
CI run claiming an agent's format was checked when nothing was read. So a
*named* prefix matching no session exits non-zero, kept apart from `is_clean()`
because it is not a drift finding; having no sessions at all still exits zero.

**It found two real defects on its first run**, which is the argument for having
built it: 774 sessions, 126,330 events, 4.3 s, 99.60% fidelity, three findings.
Codex `web_search_call` items are unparsed (CT-037) and `journal.jsonl` is being
discovered as a session (CT-038). Both are filed rather than fixed here, because
a detector and the defects it detects are different commits.

### CT-037 · Codex `web_search_call` response items are unparsed
`status: done` · `tier: A` · `size: S` · `source: CT-019`
**Why:** 38 events across 3 local sessions, found by the first `ct doctor --dir`
run. `response_item/web_search_call` is a real API item that occupied context,
so every turn containing one under-accounted.
**Done when:** the item is classified as a tool call, sized from its action, and
labelled with the query it ran, following CT-034's argument-name approach rather
than a hardcoded shape. ✔ `ct doctor --dir ~/.codex` now recognises every event
type in all 63 local sessions.

**The labelling half needed no new code at all**, which is the strongest test
CT-034's design has had. A web search carries no `name` and no `arguments`; what
it did lives in `action`, shaped `{type: "search", query, queries}` (26 of 38) or
`{type: "open_page", url}` (10). `tool_target`'s key list already ranks `url`
above `query`, so `describe(&action)` names both shapes correctly without this
module having heard of web search. The 2 remaining actions carry nothing but
their type, and get the bare tool name rather than an invented label.

**The action is sized as a proxy, not counted as text.** `query` and
`queries[0]` are usually the same string, so counting both over-counts and
counting one may under-count, and nothing in the log says which the API replays.
It is therefore `Component::Opaque` — sized from its serialized length and
excluded from `--exact`, the same rule CT-035 set for structured tool output.
The tool name `web_search` is derived from the item type, because the payload
carries none.

**One thing found while fixing it, recorded rather than acted on:** no
`web_search_call_output` item exists anywhere in the local corpus. The results
the model read are not in the log, so they are real context that lands in the
unattributed remainder. Saying more than that would be guessing at how the
harness replays them.

### CT-038 · `journal.jsonl` is discovered as a Claude Code session
`status: done` · `tier: A` · `size: S` · `source: CT-019`
**Why:** found by the first `ct doctor --dir` run — 466 unrecognised events
across 4 files. `subagents/workflows/<id>/journal.jsonl` is workflow bookkeeping
(`{"type":"started"|"result"}`), not a conversation, and discovering it meant
`ct sessions` listed four entries called `journal`.
**Done when:** the file is not discovered as a session, by a rule stated from
what the format actually is rather than fitted to one filename. ✔ The full sweep
is now 770 sessions, 126,130 events, **100% fidelity**, exit 0.

**Two obvious rules were wrong, and measuring caught both before either shipped.**

*Require a UUID filename.* Would have discarded 625 of the 711 local Claude Code
sessions: subagent transcripts are named `agent-<hex>.jsonl` and are real
sessions worth inspecting.

*Require a `uuid` on the first line.* Would have discarded 90, because that many
sessions open with a sidecar line — `last-prompt`, `mode`, `queue-operation`,
`ai-title` — carrying no `uuid`.

**The rule that survives is stated from what a session is.** Claude Code
reconstruction is an ancestor walk over `uuid`-keyed events, so a file with no
`uuid` *anywhere* has no node the walk could start from — discovery offering it
would be offering a file the adapter cannot use. Measured: the first
`uuid`-bearing line sits at depth 1 in the median case and 10 at the worst
observed across 707 sessions, while the four journals have none at any depth.
The separation is not marginal.

**Scanning the prelude also fixed a bug it exposed.** Header fields were read
from line 1 only, so those same 90 sidecar-opening sessions had no `cwd` and no
timestamp — 74 of 707 carried no `started_at` at all, and `ct sessions --since`
deliberately waves timestamp-less sessions through, so every one of them leaked
past every date filter. Each field now comes from the first line that has it,
which the append-only format makes the earliest such line. `started_at` missing
went 74 → 0. Discovery over 707 sessions went 0.34 s → 0.57 s, about 0.3 ms
each, because the scan stops as soon as all three questions are answered and the
ordinary session answers them on line 1.

**It fails open on exhaustion, which is the part worth keeping.** The prelude
scan is capped at 1 MiB, and hitting that cap establishes nothing — a session
whose first line is one enormous pasted message is merely unread, not disproved.
Only a file read to its end without a `uuid` is *known* not to be a transcript.
Discarding a real session would be a worse error than keeping four journals, so
the uncertain case keeps the file.

### CT-020 · `--format ndjson` export
`status: done` · `tier: B` · `size: S` · `source: IDEAS.md §2`
**Why:** gets most of the analytical value of a database export while keeping
the dependency tree auditable. See CT-032, which it replaces.
**Done when:** turns and context items stream as NDJSON with stable tagged
types. ✔ `ct export <id>` — session, turn and item records, externally tagged
on `type`, with a `schema` on the header line so a script can refuse a file it
does not understand rather than misread one.

**The residual is emitted as an item row, and that is the whole design.** A
consumer's first query is `SELECT sum(tokens) GROUP BY turn`. If item rows are
all that exist, that sum silently disagrees with the prompt size the agent
reported — on the worst local turn by 53% of the context. An export that invites
the mistake would undo the tool. So the remainder is a row in the category the
terminal views already print it under, *and* the turn row carries
`total_tokens` / `accounted_tokens` / `residual_tokens`, so the naive query is
right and the two ways of asking cross-check each other. Verified on the largest
multi-turn Codex session: 1,274 turns, 362,218 records, **zero turns where the
item rows failed to sum to the reported total**.

**Every token figure carries its confidence**, and a calibrated one keeps its
`raw_estimate`. Dropping provenance would launder a guess into a measurement one
`SELECT` later, which is exactly what the type system prevents inside the
process — an export is where that guarantee is easiest to lose.

**Previews are excluded.** Labels carry paths, commands and search queries,
because a size with no name is not analysable. Conversation content does not go
in: an export is a file that leaves the agent's own directory, the brief asks
for care precisely there, and redaction is CT-025 and not built yet.

**It streams.** Records go to the sink as they are produced, so memory stays
proportional to one turn — the largest local session is 362,218 records and
105.6 MB, written in 2.3 s. The sink stays in `ct-cli` because which bytes go
where is presentation; `ct-application` names no writer and takes `serde_json`
only as a dev-dependency, so no JSON codec enters its shipped graph. A broken
pipe ends the export quietly, because `ct export <id> | head` is the first thing
anyone tries and it must not report an error for working as asked.

**No `--exact`,** for the same reason `ct trace` and `ct residual` have none:
this sweeps every turn, so a per-turn opt-in would multiply by turn count.

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

---
