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

### CT-073 · Let an agent read the context it just lost
`status: next` · `tier: A` · `size: L` · `source: product decision`

**Why:** compaction does not delete anything — `replacement_history` and every
pre-compaction event stay in the append-only log, which is the only reason the
autopsy in CT-047 can exist. What compaction destroys is the *agent's* access.
So the record survives and only the user can read it, while the one party who
needs it cannot. Exposing ContextTrace read-only over MCP closes that: after an
eviction the agent asks what it had at turn N, or for one item back, and
compaction stops being lossy in the way that matters.

**This is CT-033 without the reason CT-033 was dropped.** That entry died
because replaying to a model needed an HTTP client, and the absence of any
network-capable crate is the structural form of "nothing leaves this machine".
MCP speaks stdio to a process the user already launched, so the feature arrives
and the property survives intact. That distinction is the whole basis for
reopening it and should be checked before any transport work begins.

**Done when:** an MCP server exposes the existing read-only queries; every
figure crosses the boundary carrying its confidence; a refusal crosses as a
refusal rather than as an empty result; and no new network-capable crate enters
the dependency graph.

**The risk that must be designed for, not discovered.** These tools hand session
content back to a model. `ct secrets` exists because credentials land in these
logs, so a naive `recover this item` is a mechanism for feeding a leaked key
straight back into a context window — and, through the agent, potentially into a
tool call that transmits it. Recovery must run the existing redaction path by
default, and the decision to return raw content must be explicit and recorded.
An MCP surface that laundered `[REDACTED:github-token]` back into a live
credential would be strictly worse than the compaction it exists to undo.

### CT-075 · Answer from the archive only where the log is gone

`status: next` · `tier: B` · `size: M` · `source: CT-074`

**Why:** CT-074 built the write path. Nothing reads from an archive yet, so a
copy taken today survives its log but cannot be opened after the log is deleted
— which is the only situation the archive exists for. This closes that.

**Done when:** a session whose log is gone can be inspected through every
existing read command; where a log is present it stays authoritative and the
archive is never consulted; and every view that answered from a copy says so,
naming when it was taken and whether anything was replaced on the way in.

**The risk is the whole item.** "A stale or partial copy that silently
substitutes for evidence is this project's central failure mode wearing a
database schema" — CT-074's own words. The failure is not an archive that cannot
answer; it is one that answers and does not say it was the one answering. A
redacted copy in particular will not reproduce a token count taken from the log,
so a view that presented one as the other would be laundering a transformation
this project exists to make visible. `ArchiveEntry::differs_from_source` already
carries the flag that keeps that stated rather than discovered.

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

**2026-08-08 — install/removal review, prompted by CT-074.** Reviewing the
install and removal path against the new archive found a release blocker and
fixed it. `ct archive` defaulted to `%LOCALAPPDATA%\ContextTracerchive`, which
is *inside* the desktop app's install directory: Tauri's NSIS bundler installs
per-user to `%LOCALAPPDATA%\<productName>`. It survived removal only by
accident — the generated uninstaller ends in `RMDir "$INSTDIR"`, which spares a
non-empty directory — so a template this project does not control was one
`RMDir /r` away from making "uninstall the app" delete the only surviving copies
of sessions whose logs were already gone. The default is now
`%LOCALAPPDATA%\ContextTrace-archive`, a sibling that no uninstaller owns, and a
test pins the two names against nesting so a tidier-looking default cannot
reintroduce it.

The consequence is documented rather than left to be discovered: removal leaves
archived sessions behind, that directory holds session content including
credentials if anything was archived with `--raw`, and deleting it is the user's
call. README and MVP-STATUS say so, and the `ct roots` block was re-captured from
a real run rather than hand-edited.

**Verified on this machine, from the committed workflow and a release build:**
MIT `LICENSE` present and copied into the CLI archive; the release job refuses a
tag that disagrees with `tauri.conf.json` and refuses mixed workspace versions
before building anything; assets are the per-user installer, the CLI zip and a
SHA-256 manifest over both; the release is created as a draft; signing is gated
behind complete configuration; `ct --version` reports `ct 0.1.0` and the binary
exposes fourteen commands.

**Still requires a human at a machine this session did not have.** A clean
Windows host that has never had ContextTrace installed; install, upgrade and
uninstall exercised from *downloaded* assets rather than locally staged ones,
with hashes compared against `SHA256SUMS.txt` first; a candidate soak; and the
signing certificate, which is a purchasing decision rather than a task. None of
these can be honestly closed from here, and the item stays `next` because of
them — not because anything above is outstanding.

### CT-077 · Move CI to the self-hosted Woodpecker instance
`status: next` · `tier: A` · `size: M` · `source: infrastructure, CT-062`

**Why:** all four CI jobs ran on GitHub's `windows-latest`, and three of them
had no reason to. A self-hosted instance already exists at `ci.aneskurtovic.com`
with a Linux agent, and this repository is registered on it as id 5.

**Done when:** every gate `.github/workflows/ci.yml` enforced is enforced by
`.woodpecker/`, on an agent that reports it, and that file is deleted — and a
step that deliberately fails has been shown to turn the pipeline red on that
agent, so "the smokes passed" is known to mean more than "the smokes did not
report a failure".

**Landed.** Three workflows: `frontend.yaml` and `rust.yaml` on the shared Linux
agent, `windows.yaml` on the owner's machine via the `local` backend. All three
lint clean under `woodpecker-cli v3.16.0`. See [docs/CI.md](docs/CI.md).

**Green on the real box.** Pipeline 5/1 (push): all nine steps success in 137s
cold — `frontend` 3/5/13/1s, `rust` 2/10/27/44/28s, the two workflows serialised
because the agent runs one at a time. Pipeline 5/2 (the PR) reports `frontend`
pass, `rust` pass, `windows` **pending with no agent**, which reproduces the
predicted failure mode exactly: a missing Windows agent stalls the pipeline
yellow rather than failing it red. `windows` is correctly absent from the push
pipeline on a topic branch, so the narrowed trigger works too.

**The tree was already portable, which nothing had checked.**
`rust-toolchain.toml` carried a comment saying nothing had ever built this
workspace off Windows. On `rust:1.97-bookworm` it formatted, linted, tested and
passed its 1.88 MSRV check with no change whatsoever; on `node:22` the frontend
ran all 75 tests and built. That is a consequence of a decision already recorded
in `Cargo.toml` — `clap` and `chrono` both carry `default-features = false`, for
their own reasons, and between them removed the last platform-coupled crates.
There is no `#[cfg(windows)]` in the tree at all. Only `ct-ui` stays
Windows-only, because it links a real WebView2 application.

**This closes what CT-062 deferred.** That entry corrected the toolchain comment
to admit only Windows was verified, and explicitly deferred rather than dropped
adding another host, on the grounds that it "would surface real failures on
hosts nobody has compiled here". It surfaced none. The comment is now a
statement about `ct-ui` and macOS rather than about the workspace.

**One security assertion could not fail, and now the check says so.** Measured
while porting: of the five credential values the smokes assert never survive a
redacted export, four appear in an unredacted one and the private key does not —
`ct export` omits `function_call_output` payloads, and that key exists only in
one. `ct secrets` finds it and the archive strips it, but that one export
assertion would have kept passing with redaction removed entirely. Inherited
from the GitHub workflow rather than introduced here. Both smokes now take a
positive control first and fail if it is empty, and the run log prints how many
credentials were actually reachable.

**The smokes are scripts now, not YAML.** `scripts/ci/*.ps1`, runnable by hand.
Three reasons: they are the security assertions and a `local`-backend step body
goes through a generated wrapper whose handling of a mid-script `throw` this
project has not measured; commit 4ff69ee is the record of what embedded
PowerShell does to a CI file when a heredoc eats its control characters; and the
owner should be able to run the bytes CI runs. Verified on Windows — all three
pass, a `throw` exits 1, and the leak assertion fires when handed a leaking
export. Two real defects surfaced that way: three-segment `Join-Path` is
PowerShell 6+ and the step shell is 5.1, and 5.1 makes redirected native stderr
terminating under `$ErrorActionPreference = 'Stop'`.

**The GitHub version assumed a pristine home directory.** It isolated
`CODEX_HOME` for the credential fixture but left `CLAUDE_CONFIG_DIR` pointing at
the real one — harmless on a hosted runner, wrong on the owner's machine, where
hundreds of real sessions are discovered and can push the fixture off a bounded
`ct sessions` page. Every script now isolates both homes and the archive root.

**`release.yml` stays on GitHub Actions,** deliberately. It needs
`contents: write`, `signtool.exe` and three signing secrets; moving it would
trade a scoped ephemeral token for a long-lived PAT and route certificate
material through a backend with no container isolation, to save a cost that is
already zero.

**Blocking the last step:** the Windows agent has never connected —
`last_contact: 0`. Until it does, `windows.yaml` queues as pending and the
pipeline stalls yellow rather than failing red. `ci.yml` must therefore stay
until the agent reports `platform: windows/amd64`, or this repository would have
no coverage at all for the desktop build, `ct-ui`'s Rust and the CLI smokes.

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
patterns are known before the cache is tuned. The figure this deferral rests on
was re-measured under CT-057, because the old one timed a cheaper path than the
one users wait on. On the largest local session (94.6 MiB, 88 turns) the
content-analysis path — discovery through to the first rendered turn, the
slowest route a user can reach — completes in **1.26–1.28 seconds** across three
runs on an otherwise idle machine, with the file's bytes already in the OS page
cache. A single genuinely cold observation, taken on a file this machine had not
read that day and not repeatable without dropping the cache, was **1.71
seconds**. Cached turn switches stay below 1 ms. Both figures sit inside the
2-second budget CT-044 accepted, so 0.1 still has no evidence that a persistent
index is needed — but the warm figure is the repeatable one and the cold figure
is the one a first-run user meets, and neither is quotable as the other.
**Done when:** derived metadata is cached, disposable and rebuildable, and no
domain type depends on it.


---

## Done

### CT-076 · Close the desktop's gap on the CLI
`status: done` · `tier: A` · `size: L` · `source: product decision`

**Why:** the CLI had fourteen commands and the desktop app answered nine of
them. Three capabilities were missing outright — archiving, export, and
comparing turns across two sessions — so the surface most people would actually
open was the weaker one, and the newest capability (CT-074's archive) existed
only in a terminal.

**Done when:** every measurement the CLI can produce is reachable from the
desktop, or its absence is stated rather than left to be discovered.

**Cross-session diff was not a new feature — it was making reachable what was
already written.** `ComparabilitySummary` has three variants and the desktop
already rendered all three, but `get_turn_diff` took one session and two turn
numbers, so only `identical` could ever be constructed. Two of three arms were
serialized, styled and dead. Widening the command to name two sessions is what
brought `skewed` and `incomparable` into existence on this surface; the demo
fixtures now produce all three from the same rule the domain applies, so the
arms are exercised rather than asserted.

**The pin followed the selection.** The baseline is a bare turn number, and
opening a different session carried it across — rebasing onto a session the
user never pinned, at a turn chosen for a different one and which the new
session may not even have. The reset block that clears the cross-session pick
and the last export result on selection change already stated the principle;
the pin had simply been missed. Found by using the feature in a browser, not by
a test.

**One directory, still.** The desktop's exports nest under the archive root
rather than claiming a second path, because `ct roots` says there is one
written directory and an export landing elsewhere would falsify that sentence
instead of extending it. That sentence itself was re-captured from a real run:
it used to end "only when you run `ct archive`", which stopped being true the
moment a second interface could write.

**The desktop's export redacts by default and `ct export` does not**, which is
a deliberate divergence rather than an oversight. The CLI writes to stdout —
the destination is chosen in the same breath as the command and is often a pipe
that never becomes a file. The desktop writes a durable file into a directory
it chose, beside the archive, so the reasoning that made the archive redact by
default applies unchanged. Having one subdirectory of that root default to
redacted while its sibling defaulted to raw would be a distinction nobody could
hold in their head.

**Rows that would lie about being clickable.** In a GUI a list of sessions
reads as "click to open", and nothing reads an archived session back yet —
CT-075 is that item. A row that silently did nothing would make the desktop
worse than the CLI on the same capability, and one that opened the *live*
session instead would present a copy as the thing itself. So the rows carry
exactly one control, it verifies rather than opens, and the panel says plainly
that nothing reads a copy back yet on either surface.

**Verified:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -D
warnings` at zero, 392 Rust tests and 75 frontend tests, the production bundle,
and a browser pass over every new panel — which is where the unstyled archive
list and the drifting baseline were both caught.

**Deliberately not done:** `--exact` tokenizer measurement and item filters on
the context and largest views. `--exact` is Codex-only, so it needs a control
that is disabled with a stated reason on Claude Code sessions rather than
silently inert, and that is a real piece of work rather than a flag. Left out
and said so rather than shipped thin.

### CT-074 · Archive sessions so they outlive the logs
`status: done` · `tier: B` · `size: L` · `source: product decision`

**Why:** the one thing genuinely at risk is not compaction but deletion. A log
that is rotated, pruned by the harness, or lost with a wiped `~/.codex` takes
its evidence with it, and no amount of reconstruction recovers a file that is
gone. A local archive is the only way a session outlives its log.

**Done when:** ingestion is opt-in and explicit, appends from the logs without
writing to them, records per record where it came from, and can be rebuilt for
any session whose log still exists. A session present in both must read
identically from either.

**Three constraints that are the design, not caveats on it.**

*The archive must not become the source of truth.* Where a log is present it
stays authoritative and the archive is a cache; only where the log is gone does
the archive answer, and it must say so, because a stale or partial copy that
silently substitutes for evidence is this project's central failure mode wearing
a database schema.

*It concentrates secrets by construction.* Credentials demonstrably reach these
logs — the CI fixture carries five and a single real scan found six occurrences.
Scattered JSONL under two home directories is an awkward target; one file
holding every prompt, tool output and credential you have ever produced is not.
Ingestion must run the secret scan and must default to storing redacted, with
raw retention an explicit, recorded choice rather than the path of least
resistance.

*It changes what the README's Privacy section has to say.* **Correction to this
entry as filed:** the README carries no sentence reading "ContextTrace keeps no
copy of your content". Its two live claims are "**Read-only.** Agent directories
are inputs. ContextTrace never writes to them" and "session data stays on the
machine", and an archive falsifies **neither** — it writes to a ContextTrace-owned
directory, not an agent directory, and nothing leaves the machine either way.

So the required change is an *addition*, not an edit: the section must say that
ContextTrace now writes a copy somewhere, name where, and say what is done to
credentials on the way in. It still has to land in the same commit that makes a
copy exist. And because `AgentAdapter::roots` exists precisely so the tool can
state every local path it touches, the archive root belongs wherever roots are
already surfaced — this is the first path ContextTrace *writes*, which makes
naming it more important than naming the ones it reads.

**Accepted.** `ct archive <id>` copies a session's records into a
ContextTrace-owned directory; bare `ct archive` lists what is held and
`--verify` re-digests a copy against its source. Ingestion is explicit, streams
rather than loading a session into memory, and replaces any existing copy, which
is what makes an archive rebuildable while its log survives.

**The clause that shaped the whole design is "reads identically from either".**
Both adapters are path-driven: `discover` yields a descriptor carrying a path and
`load` parses that path. So the archive stores the agent's *records* rather than
this tool's conclusions, and an archived session is parsed back by the same
adapter through the same parser. That makes identical reading a structural
property — the same bytes through the same code — instead of a claim two code
paths would have to be kept agreeing on. It also means a later build with better
reconstruction gets better answers out of copies taken today.

**Redaction is a no-op on a session holding no credentials**, which is most of
them, so the tension between "store redacted by default" and "reads identically"
is narrower than it looks: a clean session's copy is byte-identical to its log,
verified on both agents' fixtures by comparing bytes rather than parsed output. A
session where the scanner fires diverges, and the entry then states how many
records and values were replaced rather than softening it into a mode label.
`ArchiveIntegrity` is four cases because they call for different actions: intact,
a changed source (re-ingest), a damaged copy (the previous answer cannot be
trusted), and a vanished source — the case the feature exists for, and the one
where reporting a bare "verified" would be worst.

**A corrupting redaction was found and fixed before this shipped.** `redact_text`
was written for `ct export --redact-secrets`, which hands it one already-parsed
field; `find_private_keys` claiming everything to the end of its input when a PEM
block never closes is right for that caller (CT-049). Handed a whole JSONL line
it claimed the record's own closing quote and brace, so a truncated key block
produced an archived record that no parser could read — worse than no archive at
all. The archive now redacts inside each JSON string separately, which puts the
same rule at the right boundary and leaves everything outside a string untouched,
preserving the byte-identity above. A subagent found this, refused to work around
it, and left a failing reproduction rather than a passing test; that test now
passes.

**The measurement is only as good as where it is stated.** `ct roots` now names
the archive directory as a write location, listed whether or not anything has
been archived, because `AgentAdapter::roots` exists so this tool can state every
local path it touches and this is the first one it writes. The README's Privacy
section says the same, in the commit that made a copy exist. CI archives the
credential fixture and greps the bytes on disk — manifest included — for all five
known fake secrets, and asserts every archived record still parses as JSON.

**Not in this wave, deliberately:** nothing *reads* from the archive yet. The
resolution policy — log wins, the archive answers only where the log is gone, and
says so — is where "a stale copy silently substituting for evidence" lives, and
it touches every surface. Filed as CT-075 so this wave could land without the
source-of-truth risk being possible at all.

### CT-072 · Show the context the agent never logged, and when the harness changed
`status: done` · `tier: A` · `size: M` · `source: desktop product strategy`

**Why:** the unlogged remainder is this project's signature measurement — the
system prompt and tool schemas the agent never wrote down, recovered by fitting
each session's own characters-per-token ratio — and it is reachable only from
the CLI. The desktop reports a residual figure inside a single turn's
composition and stops there, so the one view that makes the measurement legible
is the one the app does not have.

The chart is the smaller half. `residual_steps` already detects *step changes*
in that remainder, and a step means something the agent did not log has changed
size: a tool was registered, an MCP server connected, a skill loaded. That is an
inference about the harness drawn from arithmetic on the transcript, and nothing
else in either agent's own tooling surfaces it.

**Done when:** a session shows its unlogged remainder across turns with the
fitted ratio and its spread stated beside it; step changes are marked and
readable as harness events rather than as noise; every figure carries its
confidence, and a session whose ratio cannot be fitted says so instead of
drawing a line through nothing.

**Not fabricating the fit is the whole risk.** Roughly one Claude Code session
in seven reconstructs to more content than the prompt held, so the constant
comes out negative and the fit is refused — `docs/methodology.md` says so and
says the cause is not established. The panel must render that refusal as the
answer, not fall back to a plausible-looking curve.

**Accepted.** `get_residual` returns a four-case sum type — `fitted`,
`overCounted`, `agentNotFitted`, `insufficientGrowth` — and the panel renders
each on its own terms. The discrimination that took the most care is the one the
warning above names: `derive_ratio` returns `Some` for a session that
over-counts, with a negative constant reported as unknown, and every per-turn
remainder in that session's series comes back unknown for the same reason.
Routing it into `fitted` would have put a measured characters-per-token figure
above an empty chart — the refusal rendered as an absence rather than as the
answer. So a series is `fitted` only when at least one turn has a readable
remainder; a session where none does is `overCounted` whether or not a ratio was
fitted. That test is a pure function over the series, checked once and tested
without needing a session that reaches the state.

The ratio never appears without the spread and sample size that qualify it, and
a turn whose remainder is unknown breaks the line rather than dropping to the
axis — a zero there would assert the complete inventory this measurement exists
to avoid claiming, so the count of such turns is stated instead. Steps carry
`nearCompaction`, decided on the backend against the session's own compaction
events, and the rise/fall explanation is withheld when every step already has a
cause in the log, mirroring the terminal view: a step a compaction explains must
not also be narrated as an unrecorded harness change.

The measurement is over the whole session, so the panel is user-triggered like
the doctor scan rather than run on every session click — producing the series
reconstructs every turn. The session cache now holds the whole derived ratio
rather than its chars-per-token field alone, so one sweep per session load serves
every view instead of one sweep per view.

**What the browser caught that the tests could not.** The first demo series held
each level perfectly flat between steps, while the panel's own caption says one
fitted ratio cannot describe a session that starts as prose and ends dominated by
tool output. Every test passed against a shape no real session produces, and a
step detector whose whole job is surviving drift was being demonstrated against a
line with none. The demo now drifts and wobbles, and each step's levels are
recovered from the series by the same median rule the backend uses rather than
typed in beside it — a marker claiming a level the line never reaches is the same
defect as fabricating the fit, one layer down.

### CT-048 · Compare two turns in the desktop app
`status: done` · `tier: A` · `size: M` · `source: desktop product strategy`

**Why:** a single snapshot says what is wrong; a comparison says what changed
when the agent's behaviour changed. The application diff use case already
normalises categories, tools, residuals and measurement comparability.
**Done when:** a user can pin a baseline turn, choose a comparison turn and see
the largest category/tool/residual deltas with incompatible measurements
clearly bounded.

**Accepted on 2026-08-08.** Pinning a turn turns the timeline into a
comparison: the pinned turn is the baseline, whatever the slider or the chart
selects is the other side, and the panel recomputes as either end moves rather
than going stale until something is clicked again. Categories and tools are
ordered by magnitude, each row carrying its item or call delta beside its token
delta.

**`Comparability` reaches the screen intact, and that is the point of the
entry.** A delta the instruments alone could have produced is shown but muted
and titled, rather than hidden — withholding it would be its own claim — while a
delta larger than the bound reads at full weight. The TypeScript mirror is a
discriminated union, because the flattened `{ kind, skew: number | null }` shape
would let a missing bound fall back to `0`, and zero skew is not "unknown" but
the *strongest* comparability claim available. A test asserts the validator
rejects an `incomparable` payload carrying a skew, which is the one combination
that would launder an absence into a certainty.

**One honest limit, recorded rather than papered over.** Both sides come from
one session and a session is calibrated once, so this path always reports
`Identical` and the `Skewed` and `Incomparable` arms are unreachable from the
desktop today. They are still modelled, validated and tested at the boundary, so
a cross-session comparison lands without reopening the type — and the Rust test
asserts `identical` explicitly, so the day two separately calibrated loads start
feeding this view, it fails rather than quietly subtracting across scales.

Driven in a browser at 1440x900 rather than inferred from unit tests, which is
how two defects in the *demonstration* data were caught: item counts did not
vary with the turn, so every row reported "0 items" changed while its tokens
moved, and the tool rows were fixed constants that showed more calls at the
earlier turn than the later one. Both are fabricated figures behind a banner
that says so, and both were still wrong in a way a real session cannot be.

### CT-047 · Add a desktop compaction autopsy
`status: done` · `tier: A` · `size: S` · `source: desktop product strategy`

**Why:** Codex records literal replacement history, so ContextTrace can show
exactly what a compaction discarded—evidence Codex itself does not turn into an
inspectable view. The diff engine already exists; the prompt-growth chart
already exposes the natural entry point.
**Done when:** selecting a Codex compaction marker lists dropped, preserved and
replacement-only items with sizes and confidence; unsupported agents explain
the evidence limit without fabricating a diff.

**Accepted on 2026-08-08.** The compaction markers the growth chart already drew
are now the affordance: clicking or keying one opens an autopsy listing what
that compaction dropped, what it preserved and what the replacement introduced,
each with size and confidence. A preserved item shows **both** of its positions
— `history #4 -> replacement #0` — which is the fact CT-060 added and the one a
user cannot get anywhere else, and a footer states which list each number
indexes rather than leaving the reader to infer it.

**The refusal is the half that was worth the care.** Two different negatives are
kept as separate typed facts instead of one error string: a specific Codex
compaction whose evidence could not be read, and Claude Code, which never
records a literal replacement history at all. The second is caught as a
success-typed `Unsupported` variant rather than forwarded as a generic error, so
the panel explains the evidence limit — *"showing one anyway would misrepresent
evidence this tool does not have"* — instead of rendering an empty table that
reads as a clean bill of health.

Two things this was verified against rather than assumed. The panel was driven
in a real browser at 1440x900, both paths: a compaction rendering 4 dropped, 1
preserved and 1 added, and a Claude Code session rendering the refusal with zero
item rows. And the real corpus was swept for scale — 32 of 100 local Codex
sessions hold at least one compaction, the richest dropping 140, 540, 363 and
197 items across four events with **zero** preserved, which is why an empty
group renders as nothing at all rather than as a heading with nothing under it.
The committed fixture's 4/1/1 would never have shown that.

A serde trap is recorded at the type: `#[serde(rename_all = "camelCase")]` on an
enum renames only the variant tag, not each struct-variant's own fields, so it
is repeated per variant and a test asserts both that `historyIndex` exists and
that no `history_index` survives. The TypeScript mirror is a discriminated union
following the `ThreadRole` precedent, so a preserved item without its
replacement index cannot be written, and the validator rejects one that arrives
over IPC anyway.

### CT-069 · Codex subagent threads collapse onto their parent's id
`status: done` · `tier: A` · `size: M` · `source: CT-055`

**Why:** the Codex adapter reports `session_meta.payload.session_id` as a
session's identity. For an ordinary session that equals `payload.id` and
nothing is wrong. For a subagent thread it does not: the child carries its own
`payload.id` and inherits the *parent's* `session_id`, alongside
`parent_thread_id` and `thread_source: "subagent"`. Every thread in a group
therefore reports one id, and since `ContextTrace::resolve` returns the first
exact match without checking for a second, `ct inspect <id>` answers with
whichever file discovery reached first and the rest cannot be opened at all.

Measured on the local corpus with a read-only sweep: **590 sessions hold 564
distinct ids. 30 files collapse onto 4 ids — the worst group is 16 files — so
26 sessions are unreachable by id today.** This is not the cross-agent
collision CT-055 was filed about; it is a same-agent one, and
`resolve_in_agent` does not help. It is the CT-039 shape again: an ambiguity
resolved by iteration order and presented as an answer.

`parent_thread_id` and `thread_source` are recorded by the harness and
currently unread, so the thread structure is derivable rather than inferable —
the same standing CT-027 had. Whether a subagent thread should be listed as a
sibling session, nested under its parent, or excluded the way CT-015 excludes
Claude Code sidechains is the design question, and it should be settled before
the id is changed.

**Design settled on 2026-08-08: a subagent thread is a sibling session, marked.**
Identity becomes `payload.id`; every rollout file is listable and openable, and a
thread carries a visible marker naming its parent, so the group's structure is
stated rather than flattened or hidden. A nested or tree presentation is CT-028's
question, not this one's.

A header-only sweep of the local corpus made the choice cheaper than this entry
assumed. **`payload.id` is already unique across all 99 rollout files**, so
identity needs no synthesis — the fix is to report the field that already
identifies a file. **Every one of the 26 subagent threads names a parent that is
itself present, and no group is nested**: all 26 point directly at their group's
root, so there is no recursive structure to model. Group sizes are 2, 4, 8 and
16. Exclusion was rejected because it would drop 26 of 99 local Codex files, and
"cannot be opened" is the defect this entry exists to fix.

**Done when:** every rollout file is reachable by an identifier that names it
uniquely, a thread group's structure is stated rather than flattened, and the
corpus sweep above reports no unreachable session.

**Accepted on 2026-08-08.** A session is identified by `payload.id`, falling
back to `session_id` only when `id` is absent. `SessionDescriptor` carries a
`ThreadRole` — `Root`, or `Subagent { parent }` — so a parent id can exist only
on the variant that has one, and `thread_source: "user"` sitting beside a parent
is unrepresentable rather than merely unlikely. `ct sessions` marks a subagent
row with its parent, and the desktop shows the same in the list row and the
detail header.

**Measured, not asserted.** Against the real local corpus the CLI now lists
**100 Codex sessions with 100 distinct ids — zero unreachable**, where the old
identity rule collapsed the same files onto 74. Every session in the corpus,
both agents together, is now distinct: **596 sessions, 596 ids, no id repeated
within an agent or across two.** That last figure also closes CT-070, which
asked for exactly this measurement. The largest thread group was opened member
by member and each reports a different turn and event count, so resolving twice
to the same file would fail the check rather than pass it quietly.

The frontend mirror is a discriminated union, not `{ kind, parent: string |
null }`. The looser shape typechecks every call site through a `parent ?? ""`
fallback that can never fire — three of them had already been written — and a
branch no test can reach is a branch no reader can justify. The invalid pairing
is now a compile error, which was confirmed by writing one and watching `tsc`
reject it rather than by trusting that it would.

Two things were deliberately not done. Resolution in `ct-application` is
untouched: with ids unique, the existing exact-match path already answers
correctly, and changing it would have been a fix aimed at a defect that no
longer exists. And no tree or nesting was modelled — the sweep found every
subagent pointing directly at its group's root, so a recursive structure would
have been built for a shape the corpus does not contain. That remains CT-028's
question.

One path is defensive rather than exercised: deriving a parent from
`session_id` when a subagent carries no `parent_thread_id`. All 26 local
subagent threads carry one. It is covered by unit test and named here so it is
not later mistaken for measured behaviour.

### CT-070 · Let the CLI name the agent when two sessions share an id
`status: done` · `tier: C` · `size: S` · `source: CT-055`

**Why:** `ContextTrace::resolve` takes an id with no agent and its exact-match
short circuit returns whichever binding was wired first. CT-055 gave the
desktop `resolve_in_agent`, because a catalog row carries both halves of a
session's identity; the CLI's arguments carry only the id. The behaviour is now
asserted by test and stated in the doc comment rather than being accidental,
which is the right interim position for something with no observed incidence:
the same sweep that found CT-069 found **zero ids shared across two agents in
590 local sessions**. Filed so the decision is written down rather than
rediscovered, and ranked below CT-069, which is the same hazard with real
incidence.
**Done when:** either a cross-agent collision is refused by name with a way to
disambiguate, or the decision to leave it is recorded against a fresh
measurement.

**Accepted on 2026-08-08 — the decision is to leave it, recorded against a
fresh measurement, which is the second of the two outcomes this entry allowed.**

The measurement had to be retaken rather than cited. CT-069 changed what a
Codex session's id *is*, so the earlier "zero cross-agent collisions in 590
sessions" described an identity scheme that no longer exists — a number that
stayed true by luck would have been indistinguishable from one that stayed true
by argument. Re-measured under the new scheme: **596 sessions, 596 distinct
ids, zero shared across agents and zero repeated within one.**

So the unscoped `resolve` retains its documented behaviour of answering with the
first binding, and that behaviour is still asserted by the test CT-055 added
rather than left accidental. Refusing a collision by name would be code for a
case with no observed instance, and this project does not ship a defence it
cannot demonstrate a need for. The desktop keeps `resolve_in_agent`, because a
catalog row carries both halves of the identity and there is no reason to
discard one.

If this is revisited, the trigger is a non-zero count from the sweep above, not
a new argument.

### CT-066 · Match compaction history without a quadratic scan
`status: done` · `tier: C` · `size: S` · `source: review`

**Why:** `compare` walks the replacement array once per pre-compaction item,
comparing whole `serde_json::Value` trees for equality, so cost grows with the
product of the two histories and each comparison is a deep structural walk. It
is fine at today's sizes and will not stay fine; a fingerprint of the kind
`fingerprint.rs` already computes would reduce it to a lookup.
**Done when:** matching is linear in the size of the two histories, with the
same dropped/preserved/added answers.

**Accepted on 2026-08-08.** `compare` fingerprints each replacement item once
into a `HashMap<ContentFingerprint, VecDeque<u32>>`, then walks the
pre-compaction history once, matching by a pop from the front of a bucket. Work
is linear in the two histories instead of their product. On synthetic 5,000 x
5,000 histories the old scan took 4.52 s and the new one 0.41 s; at the sizes
this repository actually sees — a fixture compaction holds four items and two —
the difference is invisible, which is why this was filed tier C and why the entry
said it "is fine at today's sizes and will not stay fine".

**The reuse this entry assumed would work does not.** `fingerprint::value` looked
like the obvious tool and is the wrong one twice over, both found by checking
rather than assuming. Object equality on `serde_json::Value` is
order-independent, because `preserve_order` is on workspace-wide and `IndexMap`
compares pairs rather than positions — while `fingerprint::value` hashes raw
parse order, making it *stricter* than the equality it would have replaced, so a
key reordering would have silently turned a preserved item into a dropped one
plus an added one. And it special-cases a bare `Value::String` to hash its text
rather than its quoted form, so `Value::String("null")` and `Value::Null` — not
equal under `Value::eq` — fingerprint identically. That is a real collision. The
matcher therefore canonicalises object keys itself and always serialises through
`to_string`, reusing `fingerprint::text` for the hash but not the wrong equality.
`fingerprint.rs` was left untouched: its semantics are right for the
near-duplicate detection it exists for.

Duplicates were the part worth testing rather than reasoning about. The old scan
paired the Nth copy of a value on the left with the Nth unclaimed copy on the
right, in ascending index order; a plain `HashMap<_, usize>` would have collapsed
that into "first wins, the rest drop". The queue reproduces it, and the test that
pins it was written first, run against the *old* implementation to confirm it
described the existing behaviour, and only then run against the new one — a test
written after the rewrite would have described the rewrite. One gap is documented
rather than defended against: `-0.0` and `0.0` are equal under `Value::eq` and
serialise differently, which valid Codex API JSON has no reason to produce.

### CT-071 · Two documented claims that cannot be re-captured
`status: done` · `tier: C` · `size: S` · `source: review`

**Why:** CT-068 re-captured every terminal block that could be re-run, and found
two that cannot. `docs/guide.md` says one item "reads as 12.3% of 73,138 there
and 2.6% of 339,687 here"; the second half still reproduces, but the first no
longer does — that session's peak turn has moved past the item's departure, so
`ct largest` does not show it at all any more. Separately, the `web_search_call`
fragment illustrating CT-037 has a column narrower than the binary's own padding,
and names no session or turn, so there is nothing to re-run. Both are small, and
both need an editorial choice — pick a new example, or drop the comparison —
rather than a capture.
**Done when:** each claim is either re-grounded in output that can be reproduced
today, or removed.

**Accepted on 2026-08-08.** Both claims are re-grounded in commands that can be
re-run, and neither needed the editorial retreat this entry allowed for.

The "12.3% of 73,138" comparison was recoverable once the reason it broke was
read properly: the item had not moved, the session's *peak* had, drifting past
the item's departure so that `ct largest` with no `--turn` no longer showed it.
Pinning turn 4 — the item's own entry turn, named by the trace block directly
above — reproduces both original figures exactly, and the prose now names the two
turns instead of relying on a default that has stopped meaning what it said.

The `web_search_call` fragment turned out to be reproducible too. A search of the
local corpus found the same item id and label the fragment already named, so only
its numbers and column width had been fabricated around a real row. It is now a
full captured block in the same filtered form the guide uses elsewhere, which
means it is verifiable where before it was unverifiable by construction — the
stronger outcome of the two this entry offered.

All five documented blocks across both files are now checked line-by-line against
fresh captures, and the check is part of how a doc change is accepted rather than
something remembered.

### CT-057 · Benchmark the path the desktop actually runs
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** `desktop_perf.rs` times `runtime.app.load(&id)`, but the doctor view
and `ct context` both call `load_with_content_analysis`, which additionally
SHA-256s and DEFLATEs every model-visible payload. CT-029 cites the resulting
1.43-second cold load as the evidence that no persistent index is needed, so a
deferral decision currently rests on a measurement of a cheaper path than the
one users wait on.
**Done when:** the example measures the content-analysis load as well, and
CT-029's note quotes whichever figure corresponds to the slowest path a user
can reach.

**Accepted on 2026-08-08.** The example now times
`load_with_content_analysis` beside the plain `load`, and reports both rather
than replacing one with the other — the comparison is the finding. The
content-analysis load runs first, while the page cache is least warm, because it
is the heavier path and the one a deferral decision rests on; the plain load runs
second and is labelled `load_plain_warmcache_ms` so nobody reads a warm figure as
a cold one. The old `cold_total` is gone: with two loads in the file it named a
quantity that could no longer be attributed to either, and
`content_analysis_path_total_ms` replaces it, stopping at the first rendered turn
because switching turns afterwards is a separate user action.

**The correction is larger than the label.** CT-029 cited 1.43 seconds as
evidence against a persistent index; that number came from a total whose load leg
was the *cheap* path. Re-measured, the slowest route a user can reach is
1.26–1.28 seconds across three runs on an idle machine with the file already in
the page cache, and 1.71 seconds on a single genuinely cold observation. Both sit
inside the 2-second budget, so the deferral survives — but it now rests on the
path being deferred. CT-029's note and `docs/MVP-STATUS.md` were rewritten to the
new figures; CT-044's acceptance note keeps its original numbers, with a dated
correction appended, because an acceptance record describes what was measured
then and editing it would falsify the history it exists to hold.

The example cannot force a disk-cold read, and now says so in place of implying
otherwise. Specific timings were deliberately kept out of the module comment and
put here instead: a figure in a comment is never re-measured, and this file's
whole purpose is to be re-run. An intermediate reading of 2003 ms did not survive
re-measurement — it was taken while three agents were compiling in the same tree,
and every other leg of that run was inflated too.

### CT-058 · Let `ct context` skip content analysis
`status: done` · `tier: B` · `size: S` · `source: review`

**Why:** `Command::Context` was switched to `load_with_content_analysis`
unconditionally, so the most-used command in the CLI now hashes and compresses
every payload in the session whether or not the duplicate and low-information
sections find anything worth printing. There is no flag to decline it.
**Done when:** the extra measurement is opt-in or demonstrably cheap enough not
to be, with the cost recorded either way.

**Accepted on 2026-08-08.** `ct context --no-content-analysis` skips the
fingerprint and compression pass. The default is untouched, deliberately and not
incidentally: another agent was re-capturing quoted `ct context` output from the
real binary into `docs/methodology.md` at the same time, and inverting the
default would have invalidated that capture with no file conflict to warn either
side. An opt-out flag and an opt-in flag satisfy this entry equally; only one of
them was safe to land this week.

Declining the analysis does not quietly delete the sections it feeds. Both report
every item as unmeasured, which is the same sentence CT-059 added for the partial
case — so the flag makes the report cheaper without making it quieter, and the
two features turned out to need each other.

The measured saving was about 0.13 s of a 0.81 s run on an 11.7 MB session, and
it grows with payload size. That figure is recorded here rather than in the
`--help` text. The first draft put it in the help string and called that session
"the largest local session", which it is not — the largest is 94.6 MB, as CT-057
established the same afternoon. A number in a help string is never re-measured
and cannot be corrected by re-running anything, so the help now describes what
declining costs and leaves the arithmetic here.

### CT-059 · Say how much content the doctor could not measure
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** `find_duplicate_content` and `find_low_entropy_content` both skip
items whose `content_measurement` is `None`, and nothing downstream reports how
many were skipped. The secret scan sets the right example — it carries
`scanned_records` and `unreadable_records` — but "no duplicates found" from the
same report may mean nothing was measurable. This project states its evidence
limits everywhere else; here it states a clean bill of health instead.
**Done when:** both detectors report the number of items they could not
measure, and the CLI and desktop surfaces show it.

**Accepted on 2026-08-08.** Both detectors' surfaces now carry the number of
items they could not examine. `FilteredView::unmeasured_items` counts over the
same matched set that `duplicate_content` and `low_entropy_content` rank, so the
caveat always describes the answers printed beside it, rather than describing
items those sections never considered. It reaches the terminal, `--json` through
`CompositionReport`, the desktop's `DoctorReport`, and the doctor panel.

The rule is that the count comes from whatever set the sections beside it were
computed over — not that every caller reaches it the same way. The desktop
counts over the whole snapshot, which is correct there and not an inconsistency:
its doctor runs `ContextSnapshot::duplicate_content`, which applies no filter, so
the snapshot is its matched set. On the CLI path, where a filter can be active,
the two sets diverge and only the view's answer is right.

**`--json` was the surface that mattered most and was nearly missed.** The first
implementation added a free function and called it from the two terminal
renderers, leaving `composition_report` — the thing `--json` serialises — without
it. The terminal even tells users "all groups are in `--json`", so the
machine-readable output would have been the one place a consumer could read an
empty duplicate list as exhaustive with nothing to contradict it. That is the
argument for hanging the count off the view rather than off a helper each caller
must remember: the forgotten caller is not hypothetical, it happened here.

The count says how much the detectors could not look at, not how much they
missed — the second question has no honest answer without the measurement that
is absent. Zero prints nothing in the terminal, because a caveat about nothing is
noise, but the structured surfaces always carry the field, since a consumer
cannot tell a zero from a field that was never wired. On a real 419,905-token
turn the answer is 84 items. The frontend test suite gained a case that feeds the
demo payload through the production validator: every other test stubs the Tauri
bridge and therefore only ever exercised the IPC branch, so demo data drifting
from the contract was invisible to all of them.

### CT-068 · Re-capture five stale terminal blocks in the extracted docs
`status: done` · `tier: B` · `size: S` · `source: review`

**Why:** the README-front-door extraction carried five quoted terminal blocks
into `docs/methodology.md` (lines 99 and 108) and `docs/guide.md` (lines 163,
237 and 238) verbatim from the original README. They show an em dash `—` where
the binary actually prints an ASCII `-`, and one carries a `tokens` word its own
header omits. These were inherited defects, not introduced by the move, but the
move changed their standing: they are no longer prose beside the README's own
quickstart, they are standalone public reference documents. On a project whose
whole thesis is that shown output is real output, hand-editing the punctuation
of a quoted terminal block is the wrong fix — it is exactly the failure this
branch already caught and reverted three times elsewhere. The correct fix is a
deliberate re-capture from the real binary.
**Done when:** all five blocks are re-captured from the binary's actual output
rather than hand-edited, and match character-for-character.

**Accepted on 2026-08-08.** All five defect sites across three blocks were
re-captured from a release binary rather than hand-corrected, and each was
verified line-by-line against a fresh capture afterwards.

**The verification caught the fix reintroducing the defect it was fixing.** Two
category rows had been nudged one space right, so the three surviving rows looked
right-aligned among themselves after the elision — but the binary aligns that
column across all ten rows, including the seven the block abridges away. It reads
as a rounding of the truth toward tidiness, which is the exact move this entry
exists to forbid, and it is invisible to every gate the project runs. The
verification is a script that requires every non-elided line of a documented
block to appear byte-identically in a fresh capture, and it is what turned an
assertion into a check.

Figures moved where the corpus moved, and each was confirmed rather than assumed:
the fitted ratio fell from 2.42 to 2.39 as the session's usable turn-pairs
shrank, while the observed total and residual stayed pinned; `ct largest` with no
`--turn` now answers about turn 80 rather than 59, because the session grew. The
`ct doctor --dir` block four lines away was deliberately left alone — it is
labelled as a record of a past state, and re-capturing it would have destroyed
the evidence its own sentence depends on.

**A sixth site turned up only because the check was run over every block rather
than the five named ones.** `ct growth` in `docs/guide.md` was two defects, not
one: the session had grown from 889 turns to 994, changing the sparkline, the
peak column width and the turn labels — and the block had silently dropped two
of the five "Largest changes" rows the binary prints, with no elision marker to
say so. An unmarked abridgement is the same failure as a hand-edit: it shows a
reader a complete-looking answer that the binary never gave. Re-captured, with
all five rows restored. Its truncated session id is kept, as a redaction the
reader can see, and the verification treats a visible ellipsis as the marked
abridgement it is.

Two further defects were found and are filed as CT-071 rather than fixed here,
because neither can be re-captured and both need an editorial decision. CT-071's
claim that everything re-runnable was swept is now measured rather than asserted:
the four reproducible blocks are checked line-by-line against fresh captures, and
the `ct doctor --dir` block is excluded on the record as a historical one.

### CT-051 · Widen the credential vocabulary the scanner claims to know
`status: done` · `tier: B` · `size: S` · `source: review`

**Why:** the PEM label list covers four spellings and omits
`ENCRYPTED PRIVATE KEY`, `DSA PRIVATE KEY` and `PGP PRIVATE KEY BLOCK`, so
those blocks are not detected at all. GitLab `glpat-`, npm `npm_` and bare JWTs
that arrive without a `Bearer` prefix are likewise absent. A scanner that names
a fixed set is honest only if the set is written down where a user can see it.
**Done when:** the missing key labels and token shapes are recognised, and the
documented list of what is and is not detected matches the code.

**Accepted on 2026-08-07.** The PEM list now carries seven labels, the three
missing spellings included, and `glpat-`, `npm_` and bare JWTs are recognised
alongside the existing provider prefixes. `docs/guide.md` states the full set
and, for the first time, what is *not* detected. Nothing new was special-cased
into the precedence logic: `Bearer <jwt>` still wins over the bare JWT inside
it because it starts earlier and spans further, and a provider-specific token
beats the generic assignment detector on a stable-sort tie. Both were proved by
test rather than reasoned about, which matters because they are the two places
a widened vocabulary could have started double-reporting. The JWT rule is three
base64url segments behind the literal `eyJ` header prefix, and its negative
case is a real inline source-map data URI whose embedded blob also begins `eyJ`
but carries no dots. `alg: none` tokens with an empty signature are
deliberately not detected: requiring a non-trivial signature is the stronger
structural filter, and the document says so rather than leaving the gap
unstated. `SecretFinding` still carries no matched text and still refuses to
serialize.

### CT-054 · Give the desktop caches a lifetime that fits how they are used
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** the `sessions` and `lifecycles` maps grow without bound — nothing
evicts, so clicking through a catalog retains every parsed session for the life
of the process. The only thing that ever clears them is `search_sessions`,
which clears both on every call including plain pagination, so pressing "Load
more" throws away all the parsing the user just waited for. The two policies
are exactly backwards.
**Done when:** the caches are bounded, and paging through results does not
discard sessions already parsed.

**Accepted on 2026-08-07.** Both maps are now a `BoundedCache<K, V>` with LRU
eviction at eight entries each, and `search_sessions` clears them only when the
caller asks for a refresh, which only the refresh button does. The two policies
were backwards in exactly the way this entry described: paging discarded parses
the user had just waited for, while nothing ever bounded growth. Four tests
hold both halves down: eviction order, pagination preserving the cache, an
explicit refresh still dropping a session that vanished from disk, and the
frontend sending the flag on a refresh click and not on a page. The capacity of
eight is a judgment call and not a measurement. There is no telemetry on how
broadly anyone browses a catalog, and the constant is doc-commented as a
judgment rather than left to read as derived.

### CT-055 · Identify a session by agent and id together
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** `App.tsx` keys its React list and its load-more dedup set on
`${agent}:${id}`, conceding that an id alone is not unique across agents — but
`selectedId` holds the bare id, `selected={selectedId === session.id}` matches
on it, and `api.inspectSession(selectedId)` resolves on it. Two sessions from
different agents sharing an id highlight together and load whichever the
backend resolves first.
**Done when:** selection and lookup carry the agent alongside the id, and a
test covers a colliding pair.

**Accepted on 2026-08-07.** Selection, highlighting, load-more dedup and every
per-session IPC call now carry `{agent, id}`, and the desktop's caches are keyed
on the pair.

**The half worth recording was underneath the frontend.** `ContextTrace::resolve`
took an id alone and returned the first binding's exact match, so on a real
collision the second agent's session could not be opened however it was clicked.
Checking the agent after the load would only have converted a wrong answer into
a refusal, which is better but is not what this entry asks for.
`resolve_in_agent` scopes the search before it starts, and that is what makes
the second session reachable at all. The unscoped `resolve` stays for the CLI,
whose arguments carry no agent, and now says in its own doc comment that it
answers with whichever binding was wired first rather than leaving that to be
discovered. The test opens both sides of a colliding pair and asserts their
event counts differ, so resolving twice to the same session would fail it —
an assertion on identity, not on the absence of an error.

### CT-056 · Retire the superseded `list_sessions` command
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** `App.tsx` calls only `searchSessions`, so `list_sessions` and its
`api.ts` wrapper are dead — yet the command stays registered in
`invoke_handler`, and its behaviour changed underneath its own compatibility
note. It now forwards `project` as the free-text `query`, which
`search_sessions` matches against id, path and agent as well, with
`SessionFilter.project` pinned to `None`. A caller filtering by project would
get sessions from other projects whose path merely contains the string, and its
fixed `limit: 500` truncates silently against the 816 sessions this machine
holds. Keeping a dead command whose contract quietly broke is worse than
deleting it.
**Done when:** the command is removed, or its documented project-filter
contract is restored and exercised by a test.

**Accepted on 2026-08-07.** Removed from `commands.rs`, from `invoke_handler!`
and from `api.ts`. Nothing called it: the frontend has used `search_sessions`
since it gained paging, and the documented project filter had already broken
underneath its own compatibility note. Tests that used it as setup were pointed
at `search_sessions` through a new helper rather than deleted, because they were
never testing the dead command in the first place.

### CT-060 · Separate the two ordinals in a compaction diff
`status: done` · `tier: B` · `size: S` · `source: review`

**Why:** `compare` emits one flat `Vec<CompactionDiffItem>` in which
`Preserved` and `Dropped` rows carry their index into the pre-compaction
history while `AddedByReplacement` rows carry their index into the replacement
array. Two unrelated numbering schemes share one field name, so a renderer
showing "ordinal" prints colliding values that mean different things. A
preserved item also records no position in the replacement list, so the report
cannot say where anything moved to.
**Done when:** a reader can tell which list an ordinal indexes, and a preserved
item records both of its positions.

**Accepted on 2026-08-07.** `ordinal` is gone. Each disposition now names its
own position: `Dropped { history_index }`, `AddedByReplacement
{ replacement_index }`, and `Preserved { history_index, replacement_index }` —
the second of which `compare` had been computing and then discarding. The
renderer prints `history #0 -> replacement #0` and states which list each label
indexes.

**The ambiguity belonged in the type rather than in a caption.** The JSON form
carried two rows reading `"ordinal": 1` that meant positions in different
arrays, and the terminal table never printed the field at all — so the
collision was invisible from the surface most people read and wrong on the one
they script against. Dropped, preserved and replacement-only answers are
unchanged: the existing tests still assert the same membership and now pin exact
indices, and a new test places a preserved item at different positions in the
two lists, which the committed fixture cannot demonstrate because there it sits
at zero in both.

### CT-061 · Declare the Node version the frontend requires
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** CI pins Node 22 in all three workflows, but `package.json` carries no
`engines` field and the repository has no `.nvmrc`. On Node 18 `npm ci` reports
success and then silently omits an optional native binding, so the first `npm
test` fails inside a transitive dependency with `Cannot find native binding` and
a rolldown stack trace that names nothing in this project. Verified on this
machine: Node 18.16.0 installs cleanly and cannot run a single test; the same
checkout under Node 22.14.0 passes all 22.
**Done when:** the required Node version is declared where npm will enforce it,
and an unsupported version fails with a message that names the requirement.

**Accepted on 2026-08-07.** `engines.node` is `>=22.13.0`, the strictest floor
among the installed dependencies and set by jsdom 29, with `engine-strict=true`
in `crates/ct-ui/.npmrc` so npm enforces it instead of warning, and a `.nvmrc`
beside it. Both files live in `crates/ct-ui/` rather than at the repository
root because npm reads project config from the working directory and every
`npm ci` here runs with `working-directory: crates/ct-ui`; a root `.npmrc`
would never have been read, which is the quiet way this fix could have shipped
doing nothing. Enforcement was proved by temporarily raising the floor to
`>=99.0.0`, watching `npm ci` fail with `EBADENGINE` naming the requirement,
and reverting.

### CT-062 · Verify the macOS and Linux build claim, or drop it
`status: done` · `tier: B` · `size: S` · `source: review`

**Why:** `rust-toolchain.toml` now says a host-neutral channel "keeps the
repository buildable on Windows, macOS and Linux", but every job in `ci.yml`
and `release.yml` runs on `windows-latest`. Nothing has ever compiled this
workspace on the two platforms the comment vouches for.
**Done when:** either CI builds on the platforms the comment names, or the
comment says Windows is the only verified host.

**Accepted on 2026-08-07.** The comment now says what is measured: every job in
`ci.yml` and `release.yml` runs on `windows-latest`, and nothing has built this
workspace on macOS or Linux. The two claims it already made truthfully — the
host-neutral channel, and why the Windows-GNU pin was abandoned — are kept.
Adding the two platforms to CI was considered and deferred rather than dropped.
It would surface real failures on hosts nobody has compiled here, and that is a
separate piece of work from correcting a claim that costs one comment.

### CT-063 · Pin the actions that hold write access and signing secrets
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** `release.yml` grants `contents: write`, imports a signing certificate
and publishes artifacts, while referring to `dtolnay/rust-toolchain@stable`,
`Swatinem/rust-cache@v2`, `actions/setup-node@v4` and
`softprops/action-gh-release@v2` by mutable tags; the CI Rust job goes further
and uses `@master`. Any of those refs can be moved under the repository without
a commit here. Separately, `${{ steps.signing.outputs.thumbprint }}` is
interpolated straight into a PowerShell script rather than passed through
`env:`, which is the pattern the rest of the workflow correctly uses for
`inputs.tag`.
**Done when:** release-path actions are pinned to commit SHAs, the workflow
`permissions` are narrowed to the job that needs write, and no step output is
spliced into a shell body.

**Accepted on 2026-08-07.** Every action in the write-privileged
`package-windows` job is pinned to a commit SHA with its version in a trailing
comment, `permissions` is `contents: read` at the top level with
`contents: write` on that job alone, and the signing thumbprint reaches
PowerShell through `env:` rather than being spliced into the script body — the
convention the same workflow already used for `inputs.tag`, and now used by
every step that needs a value. `ci.yml`'s `dtolnay/rust-toolchain@master` is
pinned as well, being the worst of the mutable refs.

**The remaining `ci.yml` tags were left deliberately**, not overlooked: those
jobs hold `contents: read` and touch no secrets, and this entry's condition
names the release path. Recording the boundary is the point — a narrowed scope
nobody wrote down reads later as a scope nobody noticed. Every SHA was resolved
through the GitHub API and then re-verified against it afterwards rather than
taken on trust. Pinning `dtolnay/rust-toolchain` freezes the action, not the
Rust release it installs, which is the intended reading and not a gap.

### CT-064 · Revisit the hand-written SHA-256
`status: done` · `tier: B` · `size: S` · `source: review`

**Why:** `fingerprint.rs` implements the compression function by hand and
justifies it by "a deliberately minimal Windows GNU toolchain" whose crypto
crates need MinGW libraries. The same changeset removed that constraint —
`rust-toolchain.toml` moved off the GNU pin to a host-neutral stable channel.
The rationale in the comment no longer describes the repository, so the code is
now carrying maintenance risk for a reason that has expired. The
implementation itself passes the standard vectors; this is about whether it
should still be here.
**Done when:** the hashing either moves to a maintained crate or keeps a
rationale that is true of the current toolchain.

**Accepted on 2026-08-07.** The implementation stays; the reason it gave for
staying has been replaced. The MinGW rationale expired the moment
`rust-toolchain.toml` left the GNU pin, and a comment that no longer describes
the repository is worse than no comment, because it invites the next reader to
act on a constraint that is gone. What is true now is the property CT-032 and
CT-033 were dropped to protect: a dependency graph small enough to audit and
holding no network-capable crate, which is what makes "nothing leaves this
machine" structural rather than promised.

The note also stops calling all three checks standard vectors. Two are FIPS
180-4's published empty-string and `abc` examples; the third is a
block-boundary case written here. Saying which is which costs nothing, and this
is a file whose whole justification is that it can be checked.

### CT-065 · Smoke-test the commands this release added
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** the `cli-smoke` job exercises `sessions`, `inspect` and `context`. The
three commands added since — `compactions`, `secrets`, and `export
--redact-secrets` — read raw session bytes and are the ones whose failure
matters most, and none of them runs in CI against a fixture.
**Done when:** the smoke job runs all three against the committed fixtures and
asserts something about each result.

**Accepted on 2026-08-07.** `compactions`, `secrets` and `export
--redact-secrets` now run in `cli-smoke` and assert on what they find rather
than on having exited. The compaction check requires a structurally diffable
event and at least one dropped item.

**The credential checks needed a fixture before they could assert anything.**
Neither committed fixture contained a single secret-shaped string, so a green
"no findings" would have been indistinguishable from a scanner matching
nothing — the exact failure this item exists to catch. A dedicated Codex
session now carries five fake but validly-shaped credentials under its own
isolated `CODEX_HOME`, so a shape drifting into the other fixtures cannot
satisfy it either. The job asserts a non-zero finding count, each expected kind
by name, a non-zero redaction count, the `"redaction":"secrets"` header, and in
both directions that none of the five values appears in `ct secrets` output or
in the redacted export.

The credentials sit in shell *commands* rather than only in tool outputs,
because the exported label is the field `--redact-secrets` actually touches and
tool-output preview text never reaches an export at all. Verified locally
against a release build before being trusted in CI: 6 occurrences found, 8
fields redacted, zero raw values anywhere in the output.

### CT-067 · Make the README a front door before the repository is public
`status: done` · `tier: A` · `size: M` · `source: review`

**Why:** **this gates the public launch** — it is the one moment the README is
read by people who have never seen the project, and a first impression cannot be
reissued. The file is 883 lines, and roughly 700 of them are engineering
findings: the signature regression, the negative-constant hypothesis, why two
individually well-typed halves composed into a false claim. That writing is a
differentiator and none of it should be lost; it is simply sitting where a
visitor looks for what the tool is and how to run it. The install path has the
same shape of problem — the only installation section describes downloading a
draft release that no member of the public can reach, so the first thing a new
user tries is the one thing the README does not document.
**Done when:** the README is a front door — pitch, supported agents, local
install, quickstart, command table, how to read the numbers, privacy, status,
contributing, license — with the narrative moved under `docs/` (formats,
methodology, architecture) and linked rather than deleted; the desktop app is
shown with at least one screenshot; and someone who has never seen the
repository can install and run both surfaces from the README alone.

**Accepted on 2026-08-01.** The README is down from 936 lines, and every
remaining line pulls its weight: pitch, supported agents, a features table,
local install (CLI and desktop, from source, since there is no public download
yet), a quickstart with real captured output, the command table, how to read
the numbers, privacy, status and contributing. The engineering narrative moved
to four linked documents rather than being cut: `docs/guide.md` (a worked
example per command, with real output), `docs/formats.md` (what Codex CLI and
Claude Code actually write to disk), `docs/methodology.md` (how two agents'
logs become comparable numbers, and where the tool refuses to answer), and
`docs/architecture.md` (crate layout, the invariants the type system enforces,
fixtures and testing). A screenshot of the desktop app's overview panel now
sits right under the pitch, so a visitor sees the product before reading a
word of prose. What runs today (Features) is kept separate from what is next
or deferred (Roadmap), so a reader never has to guess which claims are shipped
and which are aspirational. Test and event counts are not stated in prose
anywhere in the README or the four docs — CI publishes and enforces them, and
a hand-written count would go stale the next time a test is added; the one
corpus figure that does appear (session/event counts from a format-drift
sweep) carries its capture date, the same discipline `docs/MVP-STATUS.md`
already applied. Someone who has never seen the repository can now go from
the pitch to a running `ct.exe` using only the README.

### CT-049 · Redact a private key whose block never closes
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** `find_private_keys` locates `-----BEGIN <label>-----` and then looks
for the matching `-----END` marker; when it does not find one it falls back to
`unwrap_or(after_header)`, so the match covers the header alone and the key
body flows unredacted into `--redact-secrets` output. Truncated records are not
an edge case here — bounded and cut-off tool output is the material this
project exists to analyse, so the one input shape most likely to carry a
half-written PEM is the shape the redactor handles worst.
**Done when:** an unterminated key block redacts to the end of the record
rather than the end of its header, and a test covers a PEM cut off mid-body.

**Accepted on 2026-08-01.** An opening header with no closing marker now claims
the rest of the record instead of ending at itself, so a PEM cut off mid-body
redacts to `[REDACTED:private-key]` with nothing of the key left behind. The
trade is stated where the decision is made: prose that merely quotes a
`-----BEGIN` line is over-redacted from that point on, which is the affordable
direction. A test built from a truncated tool-output record covers it, and the
README now describes the behaviour.

### CT-050 · Find secret assignments in JSON, not only in shell syntax
`status: done` · `tier: A` · `size: M` · `source: review`

**Why:** `find_secret_assignments` only recognises `NAME = value`; it requires
a literal `=` after the name. Every session this tool reads is JSONL, where the
same fact is written `"api_key": "…"` with a colon. The generic detector
therefore never fires on the project's own corpus, and only the fixed provider
prefixes catch anything. `secretish_name` compounds it by keying on
underscores, so `apiKey`, `authToken`, `accessToken` and `clientSecret` are all
invisible.
**Done when:** colon-separated JSON members are recognised alongside `=`, name
matching does not depend on underscores, and both are covered by a test built
from a realistic session record rather than a synthetic `.env` line.

**Accepted on 2026-08-01.** A quoted key closes before its separator, so the
scanner now steps over the closing quote and accepts `:` as well as `=`. Names
are matched by word rather than by underscore: `AWS_SECRET_KEY`,
`aws-secret-key` and `awsSecretKey` reduce to the same words, and `apiKey`,
`authToken`, `accessToken`, `clientSecret` and the `x-api-key` header shape all
match. `KEY` alone still is not enough to carry the claim. The new test runs on
a JSONL tool-use record and asserts the four values are replaced while
`max_tokens`, a model name and a placeholder are left alone. Placeholder and
minimum-length filtering are unchanged; the 8 secrets tests and the 127-test
`ct-application` suite pass.

### CT-052 · Say when the desktop is showing demonstration data
`status: done` · `tier: A` · `size: S` · `source: review`

**Why:** every function in `api.ts` falls back to the fabricated fixtures in
`demo.ts` when `__TAURI_INTERNALS__` is absent from `window`, and nothing in
`App.tsx` ever says so. A user looking at invented session ids, projects and
token counts sees the same chrome as a real run, under a footer that reads
"Private by design — Reads local logs." For a tool whose entire claim is
evidence over fabrication, silently substituting invented numbers is the worst
failure available. `App.test.tsx` mocks `./api` wholesale, so this branch is
also untested.
**Done when:** demo data is visibly labelled wherever it is rendered, and a
test asserts the indicator appears when the Tauri bridge is missing.

**Accepted on 2026-08-01.** `api.isDemoData()` reports the missing bridge, and
the interface says so on every surface those figures reach: a persistent bar
above the workspace naming the sessions, token counts and findings as
fabricated; the session list heading; a "Demo data" badge where "Local only"
otherwise sits; and a footer that no longer claims to read local logs while
showing invented numbers. The bar is styled as part of the layout rather than a
dismissible overlay, with the main area made a flex column so it takes space
from the workspace instead of covering it, and the floating error/warning
banners offset to clear it — CSS only; not yet checked in a rendered window. A
new
`App.demo.test.tsx` deliberately does not mock `./api` — it renders the app with
no `__TAURI_INTERNALS__` and asserts all four labels appear and the local-read
claim does not. 23 frontend tests and `tsc --noEmit` pass.

### CT-053 · Stop holding the session lock across analysis
`status: done` · `tier: A` · `size: M` · `source: review`

**Why:** `with_session` and `with_analyzed_session` take the `sessions` mutex
and then run the caller's whole closure inside it — snapshot assembly, the
lifecycle sweep, and the doctor's raw-file secret scan. Tauri dispatches
commands on separate threads, so every IPC call serialises behind whichever one
is slowest, and a doctor run over a large session blocks the session list from
answering at all. Because the closure runs under the lock, a panic anywhere in
analysis poisons the mutex and every later command fails with "the in-memory
session cache is unavailable" until the app restarts.
**Done when:** analysis runs outside the lock, and a panic in one command no
longer disables the others.

**Accepted on 2026-08-01.** Both caches now hold `Arc` handles. `cached_session`
locks only to look up or publish an entry — the parse itself runs unlocked — and
every caller analyses through the returned handle, so snapshot assembly, the
lifecycle sweep and the doctor's raw-file scan all run with no lock held. Two
threads may load the same session at once, which costs a duplicate parse and
never a wrong answer; a concurrent content-analysed entry is never downgraded by
a plain load finishing later. Lock acquisition recovers a poisoned mutex instead
of failing, because these maps are a rebuildable cache of what is on disk. Two
tests cover it: one asserts the cache is lockable while a handle is in use and
across the secret scan, the other poisons the mutex with a real panic and then
requires the list, inspection and doctor commands to answer. The 7 `ct-ui`
tests and the full workspace suite pass, with `cargo fmt` and `clippy` clean.

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
*(CT-057, 2026-08-08: that 0.79–1.43 s range was measured through the plain
load, not the content-analysis load the doctor view and `ct context` actually
run. Re-measured on the same largest session, the slowest reachable path is
1.26–1.28 s warm and 1.71 s on a single cold observation. The budget verdict
above is unchanged — the path it was checked against was not the slowest one.)*
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
