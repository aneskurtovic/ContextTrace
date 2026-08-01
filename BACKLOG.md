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

### CT-047 · Add a desktop compaction autopsy
`status: next` · `tier: A` · `size: S` · `source: desktop product strategy`

**Why:** Codex records literal replacement history, so ContextTrace can show
exactly what a compaction discarded—evidence Codex itself does not turn into an
inspectable view. The diff engine already exists; the prompt-growth chart
already exposes the natural entry point.
**Done when:** selecting a Codex compaction marker lists dropped, preserved and
replacement-only items with sizes and confidence; unsupported agents explain
the evidence limit without fabricating a diff.

### CT-048 · Compare two turns in the desktop app
`status: next` · `tier: A` · `size: M` · `source: desktop product strategy`

**Why:** a single snapshot says what is wrong; a comparison says what changed
when the agent's behaviour changed. The application diff use case already
normalises categories, tools, residuals and measurement comparability.
**Done when:** a user can pin a baseline turn, choose a comparison turn and see
the largest category/tool/residual deltas with incompatible measurements
clearly bounded.

### CT-043 · Ship an installable 0.1.0
`status: next` · `tier: A` · `size: L` · `source: MVP review`

**Why:** the CLI and desktop app work for a developer with the repository, but
the locally built installer is unsigned and has not passed a clean-machine
acceptance run. There is still no public downloadable artifact or signing
certificate. Publishing to crates.io remains outside 0.1 because the CLI's
internal path dependencies have no registry version requirements.
**Done when:** the intended license is present, the chosen release channels are
explicit, the desktop installer and checksummed CLI artifacts install on every
supported host, and the README's install/upgrade/known-limitations steps have
been exercised on clean machines.

**Progress:** the MIT license, current-user NSIS configuration, draft-first
Windows release workflow, CLI ZIP, SHA-256 manifest and operator procedure now
exist. On 2026-08-01 the unsigned installer rebuilt, replaced an existing
per-user 0.1.0 installation, registered its uninstaller and Start-menu/Desktop
shortcuts, and launched against 816 local sessions. A subsequent local
uninstall removed only the app registration, files and shortcuts; reinstalling
the same hashed candidate restored them and launched responsively. Native
1024×680 and 1440×900 checks pass. The workflow rejects mixed workspace
versions, fails closed on partial signing configuration and, when a PFX,
password and timestamp service are provided, signs and verifies both the
installer and companion CLI.
Certificate provisioning is deferred until the production-release decision;
a clean Windows host, downloaded-artifact/CLI checks and candidate soak remain.

---

## Todo

### CT-026 · Cost projection
`status: todo` · `tier: B` · `size: S` · `source: IDEAS.md §4`
**Done when:** per-category cost is derived from a local pricing table, clearly
marked as an estimate that depends on a table which will go stale.

### CT-028 · Session family trees
`status: todo` · `tier: C` · `size: M` · `source: IDEAS.md §9`
**Why:** the DAG already parsed for reconstruction contains every abandoned
branch; showing them is nearly free and no other tool does it.
**Done when:** branches are enumerated and each is separately inspectable.

### CT-029 · `ct-index` SQLite cache
`status: todo` · `tier: C` · `size: L` · `source: plan`
**Why:** deliberately deferred until a command feels slow, so the access
patterns are known before the cache is tuned. Desktop-path release benchmarks
now complete the largest local cold load in 1.43 seconds and cached turn
switches below 1 ms, so 0.1 has no evidence that a persistent index is needed.
**Done when:** derived metadata is cached, disposable and rebuildable, and no
domain type depends on it.

---

## Done

### CT-046 · Trace a contributor through the desktop timeline
`status: done` · `tier: A` · `size: S` · `source: desktop product strategy`

**Why:** the context view answers what is large now, but the next question is
when a suspicious file or tool result entered, how long it survived, and what
removed it. The lifecycle engine already answers this in the CLI, so making
largest-contributor rows open a timeline is mostly desktop presentation work
and gives the app a debugger-like interaction neither harness provides.
**Done when:** selecting a contributor shows its first/last presence, every
present interval, departure reason and compaction boundary where applicable,
with ambiguous or unavailable lineage stated rather than guessed.

**Accepted on 2026-08-01.** Every largest-contributor row is now a keyboard-
focusable trace control. The desktop runs and caches the existing whole-session
lifecycle sweep, then shows observed presence ranges, first/last turns, a
recorded compaction departure, Claude Code branch divergence, unexplained
departure, unreadable gaps and excluded main/subagent turns using distinct
language. A current 253-turn Codex trace completed in 0.49 seconds and found a
170-turn run removed by the next compaction. IPC validation, two-agent fixtures,
frontend open/close state tests and responsive 1024×680/1440×900 checks pass.
The updated unsigned NSIS bundle also completed an in-place local upgrade and
the installed app launched responsively.

### CT-045 · Put Context Doctor in the desktop app
`status: done` · `tier: A` · `size: S` · `source: desktop product strategy`

**Why:** duplicate detection, low-entropy ranking and credential-shape scanning
are valuable local-log deductions that Codex and Claude Code do not expose, but
they previously required terminal commands.
**Done when:** a user can explicitly run all three analyses from a selected
desktop turn, see actionable names and token footprints, and no secret value
can cross the IPC boundary.

**Accepted on 2026-08-01.** The new on-demand Context Doctor fingerprints and
compresses model-visible records, reports exact repeated-token footprint, ranks
large compressible blocks with the existing non-savings waste score, and scans
the full session for credential shapes while returning only type and log
location. Ordinary browsing keeps the cheap parse path; the analyzed session
is cached after opt-in. IPC validation, two-agent fixture coverage and frontend
consent/state tests pass. A current 208-turn Codex session produced 16 exact
duplicate groups, 42 low-entropy blocks and two potential-secret locations; the
underlying release analysis paths completed in 0.43 and 0.55 seconds. Responsive
1024×680 and 1440×900 checks show no horizontal overflow.

### CT-044 · Complete desktop MVP acceptance
`status: done` · `tier: A` · `size: L` · `source: product decision`

**Why:** the public product requires a desktop app that completes the core
workflow without relying on the CLI.
**Done when:** the Windows desktop build completes the discovery-to-diagnosis
workflow on real Codex and Claude Code sessions; cold load and cached turn
switching have explicit budgets and meet them on the largest local sessions;
empty, malformed and very large sessions have usable states; keyboard and
1024/1440px layouts pass visual acceptance; and an end-to-end test covers the
Rust IPC contract without reading private corpus data.

**Accepted on 2026-08-01.** Fixture-backed Rust tests cover the full desktop
list-to-inspect-to-context IPC contract for both agents, cache refresh and
failure cases. Release benchmarks on the largest local Codex session (94.6 MiB)
and two high-turn Claude sessions put cold discovery-to-snapshot at 0.79–1.43
seconds and cached turn switching below 1 ms, inside the 2-second/50-ms budgets.
Seventeen frontend tests cover loading, empty, error and malformed IPC states,
keyboard-operable chart points, focus visibility, live regions, reduced motion
and out-of-order requests. Installed acceptance then exercised search and both
agent filters against 816 local sessions and inspected native 1024×680 and
1440×900 layouts. The pass exposed 8–9 px supporting text; it was raised to
9–12 px and rechecked with no horizontal overflow.

### CT-027 · Codex compaction diff engine
`status: done` · `tier: B` · `size: M` · `source: IDEAS.md §1`

**Why:** Codex records `replacement_history` verbatim, so what was discarded is
*derivable rather than inferable* — a stronger claim than anything available for
Claude Code.
**Done when:** the exact items dropped by a compaction are listed with sizes.

**The missing boundary evidence arrived on 2026-07-28.** One retained Codex
session compacted after turns 74 and 134, with 7 and 4 literal replacement
entries. `ct compactions <id>` now compares the pre-compaction response items
with each replacement list and reports dropped, preserved and replacement-only
items without printing their content. Sizes are normalized compact JSON bytes;
wholly textual items also receive a tokenizer measurement. Missing, malformed
or oversized raw lines are reported as unavailable, and adapters such as Claude
Code that do not record a literal replacement list refuse the operation rather
than inferring it. Four focused synthetic tests cover preserved messages,
dropped tool output, opaque replacement blobs, failed raw access and recovery
across multiple compactions. On the two motivating boundaries the structural
counts are 243 dropped / 2 preserved / 5 replacement-only and 175 / 3 / 1.

### CT-042 · Automate the public-MVP verification gate
`status: done` · `tier: A` · `size: M` · `source: MVP review`

**Why:** local verification alone could not prevent a broken main branch or a
Windows-only dependency regression. Tauri also raised the real minimum Rust
version from the previously claimed 1.85 to 1.88.
**Done when:** CI runs formatting, clippy, all workspace/all-target tests,
frontend type/build/tests, desktop compilation, release builds and process-level
fixture smoke flows on the supported release hosts, including the declared
minimum Rust version.

**The first main-branch run passed the complete gate.** Windows jobs exercise
stable and Rust 1.88, all 293 Rust tests, all frontend tests and production
build, Tauri compilation, a release workspace build, and process-level Codex
plus Claude fixture smoke flows. The workflow uses locked npm dependencies and
shared Rust caches without reading the private corpus.

### CT-031 · Investigate reconstruction over-count
`status: done` · `tier: A` · `size: M` · `source: corpus`

**Why:** some historical Claude Code sessions and one historical exact Codex
case accounted for more content than the observed prompt held. A hidden
harness removal was plausible, but inventing an unlogged mechanism would make
the reconstruction less honest rather than more useful.
**Done when:** either a mechanism is identified from evidence, or the hypothesis
is written up as unresolvable from logs alone and the tool's reporting of it is
final.

**The retained evidence establishes the ceiling, not a mechanism.** A fresh
read-only audit covered 40 Codex sessions and 3,353 turns. Exact recounts at
every session maximum and every observed prompt drop checked 74 boundary turns
with zero current over-counts. The logs contain 24 compactions, all with
recorded replacements, but no generic rewind, removal, lineage link or event
kind that could explain the historical cases. The affected historical artifact
is no longer retained, so the cause is not recoverable from available logs.
Existing calibration tests and output already treat content above the observed
prompt as an unknown remainder rather than zero or a fabricated explanation.

### CT-041 · Make inline-image accounting threshold-independent
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** ordinary Codex tool outputs included inline `image_url` payloads in
their serialized-character estimate, while outputs above the 4 MiB parse cap
excluded them. The same content therefore received a different accounting
policy solely because it crossed an implementation threshold.
**Done when:** ordinary and oversized outputs use one documented image policy,
image base64 is never presented as BPE text, and fixtures cover both sides of
the parse cap.

**One policy now spans both parsers.** Inline data-image payloads are removed
from the text proxy and reported as image count plus excluded payload
characters whether the surrounding output is parsed normally or scanned above
the cap. Exact recount refuses image-bearing items instead of tokenizing
base64. Regression fixtures exercise parsed JSON, escaped data URLs, and real
lines at exactly 4 MiB and one byte above it.

### CT-030 · Build the Tauri v2 desktop vertical slice
`status: done` · `tier: A` · `size: L` · `source: plan`

**Why:** a public ContextTrace release now requires a desktop interface, not
only a capable CLI.
**Done when:** the desktop app calls the same crates through `#[tauri::command]`
with no measurement or adapter-wiring logic duplicated.

**The first slice is useful rather than ornamental.** The React interface
discovers and filters local sessions, shows model/project/branch metadata,
charts measured prompt growth with compaction markers, selects measured turns,
and renders category composition and the twenty largest contributors with
source and confidence. A disposable in-memory cache keeps turn switching from
re-reading and re-calibrating the session; refresh invalidates it.

**The composition root moved, not copied.** `ct-runtime` now owns agent and
tokenizer selection plus session calibration. Both the CLI and Tauri commands
call it, so a new adapter or measurement policy cannot silently differ between
interfaces. The desktop capability is core-only, the CSP permits local IPC,
and the frontend has no remote assets.

### CT-025 · Secret scanning and redacted export
`status: done` · `tier: B` · `size: M` · `source: brief`

**Why:** the brief requires optional secret redaction for exports, and this is
the one item where being wrong has consequences outside the tool.
**Done when:** exports can be redacted, and scanning never writes findings
anywhere outside the user's terminal.

**What building it taught. The obvious export surface was not the whole export
surface.** Message and tool-output previews were already excluded, but labels
still carry shell commands, paths and search queries. The first redaction pass
covered those labels and their structured `source` fields; its own regression
test then found the credential still present in `id`, because a context item id
may be derived from that label. Redaction now covers every user-derived string
that crosses the NDJSON boundary: session and item ids, path, project, model,
label and every string-bearing source variant. The header says
`"redaction":"secrets"` and the terminal counts changed occurrences.

**A finding cannot disclose the thing it found.** The scanner's public result
contains only a credential kind, occurrence count, turn, source line and event
type. It has no matched-text field and deliberately implements no serialization;
`ct secrets` consequently has no `--json` mode. Provider tokens, bearer tokens,
PEM private keys and secret-like environment assignments are recognised by
bounded local scans with no new dependency and no write path.

Scanning follows the same lazy-content boundary as exact counting: ordinary
commands do not re-read anything, while `ct secrets` fetches each context-bearing
record once no matter how many turns retain it. Recorded base instructions and
Codex replacement histories are included explicitly. A live Codex session
reported 19 occurrences across 196 records, all traceable to the valid-looking
credential examples used while building the feature; a 325-record Claude Code
session reported none. That is the right semantic boundary: these are
**potential secrets**, not proof of a live credential or a cloud leak.

### CT-040 · Preserve oversized context items
`status: done` · `tier: A` · `size: S` · `source: CT-023`

**Why:** `jsonl::read_lines` deliberately declines to parse lines above 4 MiB,
but an oversized non-compaction line became a `SessionEvent`, which does not
occupy context. Live Codex tool outputs therefore disappeared from the item
list and inflated the unattributed remainder.
**Done when:** an oversized response item remains represented in context with an
honest size/confidence, without treating inline image base64 as text tokens or
fully parsing the pathological line.

**What building it taught.** The backlog evidence had already gone stale. The
three recorded cases had grown to **10 oversized live outputs across three
sessions**, ranging from 4.24 MiB to 20.81 MiB. Together they held 24 inline
images and 89,895,024 image-URL characters. The two other oversized lines remain
correctly excluded `image_generation_end` telemetry.

The useful boundary is not "parse or know nothing". The JSONL reader lends the
raw line to the adapter before reusing its buffer, and a narrow lexical scan
recovers only the response-item type, call id, output shape and image URL
lengths. It never materialises a multi-megabyte `serde_json::Value`. String
tokens are skipped as units and object depth is tracked, so field-looking text
inside tool output cannot be mistaken for envelope metadata.

An oversized tool result now occupies context with observed membership and an
estimated size. Plain string output is counted in decoded characters; structured
output keeps its serialised structural/text proxy with every `image_url` value
removed. The item label states the image count and excluded payload size. Those
images therefore receive no base64-derived text-token estimate during
observed-total reconciliation. `--exact` refuses the line from its recorded byte
length before fetching or parsing it, preserving the same memory bound on the
opt-in path.

Layered regressions cover the bounded JSONL hand-off, decoded escaped text,
multiple inline images, unrelated oversized telemetry, context replay/tool-call
joining and exact recount refusal. A live turn confirmed the formerly absent
items now appear as estimated tool outputs with the exclusion stated.

### CT-024 · Waste and low-entropy detection
`status: done` · `tier: B` · `size: M` · `source: IDEAS.md §4`

**Done when:** large low-information blocks are ranked by compressed-size ratio.

**What building it taught.** Compressed size belongs on the same opt-in parse
path as exact duplicate identity. Both adapters already isolate the
model-visible payload and remove retry-specific transport ids there; compressing
that value while it is in memory measures the thing the model received without
re-reading the session or retaining a second copy. Each event keeps only its
original and compressed byte counts beside the existing hash. Commands that do
not diagnose content still pay nothing.

A compression ratio alone ranks a tiny repeated acknowledgement above a
ten-thousand-token build log. The useful order is
`tokens × (1 - compressed/original)`, the expansion already proposed in
`IDEAS.md`: size says how much prompt budget is at stake and the ratio says how
repetitive it is. The terminal calls this a **waste score**, not wasted tokens.
Compression establishes redundancy, not that removing the redundant fraction
would preserve the information the model needed.

The qualification boundary is explicit rather than intuitive: at least 4 KiB
of visible payload and a DEFLATE ratio no higher than 75%. Four KiB makes
compressor framing irrelevant and normally means roughly a thousand tokens of
code or logs. Text output shows the ten highest scores and `--json` carries
every finding, including original bytes, compressed bytes, ratio, score,
category, provenance and confidence. Existing context filters narrow the
analysis before ranking.

Two live peak-turn checks produced signal immediately. A 383,810-token Claude
Code turn contained 44 qualifying blocks with a combined score of 92,073; its
largest were captured build output, full source reads and a long compacted
summary. A 244,500-token Codex turn contained 34 with a combined score of
140,947, led by repeated large command outputs. Those totals remain scores, not
claims about recoverable context.

The first implementation used `flate2`, whose CRC dependency runs a build script
that cannot link on the pinned Windows GNU toolchain -- the same constraint
CT-023 met in the common crypto stack. The final implementation uses the
build-script-free pure-Rust DEFLATE core directly at the conventional level 6.
It adds two small, non-network-capable packages (`miniz_oxide` and `adler2`) and
keeps the repository's structural local-first guarantee intact.

### CT-023 · Duplicate context detection
`status: done` · `tier: B` · `size: M` · `source: IDEAS.md §4`

**Why:** agent retry loops re-inject identical file content, and it is invisible
in a per-turn view.
**Done when:** identical content appearing more than once in a turn's context is
reported with its total cost.

**What building it taught.** Exact matching belongs at parse time, not as a
second read of the session. On the content-analysis load path, both adapters
already hold each parsed payload long enough to measure it; reducing its
model-visible content to a SHA-256 identity there keeps the existing
lazy-content boundary intact. A context item grows by 32 bytes rather than by a
second copy of a tool result that may be megabytes. The domain sees only
equality, not JSON or an agent-specific shape.

That load path has to be opt-in. The first version fingerprinted every parsed
session, making a debug `ct doctor --dir` sweep pay to hash hundreds of
megabytes it immediately discarded: 51 seconds on this corpus. Giving the
adapter port an explicit content-fingerprint load restored the ordinary path
(19 seconds in the same debug build), while `ct context` alone pays for the
analysis it prints. A fixture test keeps that boundary from collapsing later.

Transport identity is not content identity. A retry gives a tool result a fresh
`call_id`/`tool_use_id`, so hashing the whole event would miss the exact defect
this item exists for. The adapters hash output, message text, tool arguments or
injected attachment content and exclude those linkage fields. The terminal
reports both the footprint of every copy and the avoidable cost after retaining
the first; JSON includes every group. Text output shows the ten largest and
summarises the rest, because one real Claude Code turn held 16 groups and
printing all of their 88 members buried the context composition that found them.

Refusal matters here too. Claude Code usually strips thinking text and leaves
only an opaque signature. Equal signatures do not establish equal hidden text,
so redacted reasoning carries no fingerprint and cannot become a false exact
match. Near-duplicates are likewise out of scope: whitespace and JSON ordering
remain significant.

Two live peak-turn checks found real duplicates immediately: the current Codex
session held 7 groups / 14 copies, costing 546 tokens total and 273 after the
first copies; the largest local Claude Code session held 16 groups / 88 copies,
costing 3,653 total and 2,716 repeated. Most were repeated tool acknowledgements
and commands rather than spectacular single blobs. The feature is useful even
when the bug is death by dozens of small retries.

The first implementation pulled in a standard SHA-256 crate. Its transitive
build script could not link on the pinned Windows GNU setup, contradicting the
project's deliberately minimal toolchain. The final implementation keeps the
small FIPS 180-4 compression function inside the adapter crate and checks it
against standard empty, `abc`, and full-block vectors; no dependency was added.

### CT-039 · `pick_turn` presents a fallback as a peak
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** `peak_turn` ranked turns on `prompt_tokens().unwrap_or(0)`. Where no
turn carries a usable size every key is zero, so `max_by_key` returned whichever
turn came last and `pick_turn` handed it back as "the session's largest" — a turn
nobody chose, described as one that was measured. The error message naming that
condition fired only when there were *no turns at all*, so the one case it named
was the one case it never caught.
**Done when:** a session whose turns carry no usage record is either refused by
name, or has its defaulted turn stated as a fallback rather than as a peak.

**What building it taught. The severity in the original filing was wrong, and
wrong because it was estimated rather than measured** — which is the finding
worth keeping. This entry claimed "all five affected sessions in the local corpus
have exactly one turn … nothing has been mis-sized yet". A sweep of all 775
sessions found **256**, not five: subagent sidecar transcripts, which log a turn
but no usable usage figures. Filing a severity is filing a claim, and this
project's own standard is that claims get measured.

**Nor was it latent.** CT-039 and the zero-usage defect were each analysed alone
and each looked survivable; together they were not. `prompt_tokens()` returned
`Some(0)`, `peak_turn` selected that turn, and calibration then scaled the
estimates to fit an observed total of zero. The pre-fix output, captured before
the change to confirm it rather than assume it:

```
Context at turn 1 - 0 [observed]
  Developer instructions          0    0.0%  ····················  [estimated]
  Tool definitions                0    0.0%  ····················  [estimated]
  Calibration: estimates scaled by 0.00 to meet the observed total of 0.
  The estimator ran inf% high, so the scaled figures consumed the whole …
```

An empty context, stated as **observed**, for a turn holding ~10,400 tokens. That
is the exact failure the `TokenCount` sum type exists to make unrepresentable,
and it got through because both inputs to it were individually well-typed. Two
correct-looking parts composed into a false claim.

**The fix was to have one implementation, not to fix the broken one.** "The peak"
existed three times: here, in `diagnostics.rs`, and nowhere authoritative. The
`diagnostics` copy already filtered on `prompt_tokens().is_some()` and was right;
the public one was wrong. Adding the missing filter would have left three copies
and scheduled the next divergence, so `peak_turn()` now lives in the domain next
to `peak_prompt_tokens()` — which was always correct, being a `filter_map` — and
both former implementations delegate to it. Ties resolve to the earliest turn, so
a session that plateaus reports where the plateau began.

**Refusing beat defaulting**, and the deciding argument was that nothing else in
the codebase was willing to guess either: `Diagnostics.peak_turn` is already
`Option`, and the renderer simply omits the line. A `Peak`/`Fallback` sum type
was considered and rejected — every turn-selecting call site would have to match
on it to decide whether to print a caveat, which is the same hazard as the
estimator defaulting that `SessionCalibration::effective` was written to remove.
The refusal names the way out (`pass --turn N`), and that path gives an honest
`10,372 [estimated]` where the default previously gave `0 [observed]`.

**All six commands that default through this path were then run against an
affected session**, because a shared refusal is only as good as its worst call
site. Two follow-on fixes came out of it: the message said "this session" while
`ct diff` resolves two of them, so it now names the session id — in the one
command whose whole job is keeping two sides apart; and `ct growth` returned
silently from drawing a chart it had no scale for, leaving a header with nothing
under it, which reads as "flat" or as a display bug rather than as "nothing was
measured". Neither was visible from the unit tests or from the two commands
checked first.

### CT-022 · Context growth timeline
`status: done` · `tier: B` · `size: S` · `source: plan`

**Why:** a session's context problem is usually a shape, not a number — and the
shape is invisible one turn at a time.
**Done when:** per-turn prompt size renders as a sparkline with compaction
boundaries marked.

**What building it taught.** This is the first command that reads *only* what the
agent reported about itself. No reconstruction, no estimator, no calibration —
`session.turns()` and nothing else. That constraint is what makes charting 889
turns cheap, and it is worth noticing that the cheapest command is also the one
whose every number is `Observed`.

Drawing a whole session at once surfaces artefacts that a per-turn view cannot,
because a per-turn view has nothing to look wrong *against*. Three of them:

*A turn with no usage record is not a turn of size zero.* Charting session
25e27e70 showed a vertical fall to the floor and an immediate return — twice.
The turns were real, and carried `{input: 0, cache_creation: 0, cache_read: 0,
output: 0}`. Every model request carries a prompt and a system prompt alone puts
the floor in the thousands, so that object is a record written but never filled
in, always sitting just before a large `cache_creation` with no `cache_read`:
a cache reset. First quoted from a 60-session sample as "27 turns across 3,795";
counted across the whole corpus it is **393 of 52,156 turn records, in 320 of 775
sessions** — and in both agents, 333 turns in 302 Claude Code sessions and 60 in
18 Codex ones. The Codex occurrences are not characterised and no claim is made
about their cause. Quoting a sample as though it were a census is the same error
this entry's own correction below is about. Read as measurements they invented a fall of −287,629 and a rise of
+288,312, taking **three of the five largest reported changes in that session**.
Fixed in the domain, where the meaning lives, so every command reading a prompt
size gets it.

The obvious follow-on claim — that the characters-per-token fit was also being
poisoned — was **checked and is false**, which is the more useful result. Such a
turn does enter `ratio::derive` as a sample, but both pairs it forms are already
rejected: the pair before it underflows `checked_sub` on token growth, and the
pair after it yields a ratio far below the plausible range. Its pull on the
recovered overhead constant is absorbed by a median. Measured on the five
affected sessions in a 14-session sample, ratio and overhead are **identical to
the printed precision with the filter on and off**. Two guards written for other
reasons had it covered. Worth writing down twice over: the fix is real but its
blast radius is per-turn presentation, and a plausible mechanism asserted without
measuring it is exactly what this project claims not to do.

*A compaction the agent did not place is not one that did not happen.* Rather
than attaching it to a plausible neighbouring turn, it is counted as unplaced and
reported as such. A compaction naming a turn the session does not contain is
treated the same way, which matters because a truncated or filtered session is a
normal thing to be handed.

*A column is not a turn.* At 889 turns and 60 columns, each column is 15 turns
drawn at their maximum — so a fall inside a column does not appear at all. A
sparkline that aggregates silently invites reading a smooth line as a smooth
session, so the caption states the ratio and states the omission.

The header also names the gap count, and a reported change spanning one says
`(across 1 unrecorded turn(s))` rather than presenting a two-turn delta as a
one-turn event. Widened the exposure of **CT-039**, noted in that entry.

### CT-021 · `ct diff A..B` — compare two sessions or turn ranges
`status: done` · `tier: B` · `size: M` · `source: plan`

**Why:** "it worked yesterday and fails today on the same task."
**Done when:** structural differences in composition, tool usage and residual
growth are reported side by side.

**What building it taught.** A diff between two sessions is a comparison between
two *instruments*. Claude Code item sizes come from a characters-per-token ratio
fitted per session, and across the local corpus that ratio runs 1.85 to 2.51 —
a 36% spread (re-measured over 14 sessions; the 5-session sample this entry
originally quoted gave 2.00–2.55, so the wider sample made the case stronger,
not weaker). Subtracting one session's category totals from another's mixes the
change in content with the difference between the two scales, inseparably. The
residual is worst hit, being `observed_total − sum(estimates)`: the axis this
entry names third is the one most contaminated by the measurement.

Neither obvious fix works. Averaging the ratios invents a third instrument that
matches neither side. Re-sizing one side with the other's ratio makes the deltas
clean at the cost of making that side disagree with `ct context` for its own
session — the CT-020 defect in mirror image. What works is to *quantify* the skew
and carry it to every row as the largest delta the instruments alone explain.
A row above its bound is content; a row below it is measurement. This falls out
row by row instead of being asserted once in a caption.

Writing the residual test found a defect in that bound before it shipped. A ratio
moving the items by 9% moves the residual by 9% **of the items**, not of itself,
so bounding the remainder like an ordinary row understated it by more than half
and would have presented a 3,000-token move as a finding. It is now bounded by
the accounted total — which makes the most contaminated row self-flagging rather
than the boldest one.

Cross-agent is a refusal, not a wider bound: no factor relates a measured count
to a ratio estimate, and Codex logs its own system prompt, so the two residuals
are not the same quantity. Token deltas are withheld and the counts carry the
comparison — which is why the axes are ordered by how instrument-free they are.

One incidental find: `heuristic:chars/2.17` and `heuristic:chars/2.18` render
identically, because the name formats to one decimal. Comparing instruments by
name would have called those one instrument and bounded nothing away.

Running it also surfaced **CT-039**, in the way this tool is supposed to: the
diff prints each side's total with its provenance, so a `[estimated]` total
sitting under a defaulted turn was visible on sight. Putting provenance next to
every number keeps finding things no assertion would have.

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
for care precisely there. Redaction was deliberately left to CT-025 rather than
being smuggled into this item; it is now available as `--redact-secrets`.

**It streams, and the cost is turns rather than bytes.** Records go to the sink
as they are produced, so memory stays proportional to one turn. Measuring two
sessions inverted the intuition: the 99 MB one exports in **0.46 s** (88 turns),
the 25 MB one in **2.1 s** (1,274 turns, 362,218 records, 105.6 MB out). Every
turn is reconstructed, so turn count is the driver and file size is not — which
is also the shape CT-029 will need to know when the index is built. The sink
stays in `ct-cli` because which bytes go where is presentation;
`ct-application` names no writer and takes `serde_json` only as a
dev-dependency, so no JSON codec enters its shipped graph.

**Two defects review caught, both about agreeing with the rest of the tool.**

*The export used a different estimator from every other command.* It called
`snapshot` directly, so Claude Code items were sized with the flat
`chars/3.1` default while `ct context` used the ratio fitted to that session
(CT-014). Same session, same turn, two answers: a residual of 116,872 against
64,167 — 82% apart, on the one number the tool exists to name. The estimator is
now threaded through, they agree exactly, and the header record carries its
name so a file whose numbers cannot be reproduced does not exist.

*`ct export <id> | head` printed an error on Windows.* Broken pipes were matched
on message text, and Windows reports `ERROR_BROKEN_PIPE` (109) and
`ERROR_NO_DATA` (232) without mapping either to `ErrorKind::BrokenPipe`. Now
matched on kind and raw OS error, so the first thing anyone tries stops
complaining about working as asked.

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
