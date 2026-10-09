# Notifications

The desktop watches the sessions it can already read and raises a finding when
one crosses a threshold you set. It is the only part of ContextTrace that
speaks first, so the whole design question is when it is allowed to, and what
it is allowed to claim afterwards.

There is no CLI surface for this. The rules run in the desktop app, and the
feed they write is a desktop artefact.

## The twelve rules

| Rule | What it says | What you configure |
|---|---|---|
| Context window pressure | A turn crossed a share of its context window | Warning and critical percentages, plus the levels they reset below |
| Prompt size jumped | Prompt tokens grew sharply between turns | Token growth |
| Large context contributor | One item took an outsized share of a turn | Token threshold and the share of the turn it must reach |
| Tool failures are repeating | Consecutive tool calls returned errors | Streak length |
| Context compacted | The agent compacted, and how much it reclaimed | Delivery only |
| Potential secret in model context | A credential-shaped string reached the context | Delivery only |
| Agent log format changed | This build met an event type it does not recognise | Delivery only |
| Duplicate context detected | Identical content repeated inside one turn | Token threshold |
| Low-information context | A large, highly-compressible block ranked as waste | Token threshold |
| Hidden context changed | The unlogged remainder moved between turns | Token threshold |
| Instructions changed | A recorded instruction artefact drifted | Delivery only |
| Session cost budget crossed | Estimated session cost passed a budget | Budget amount |

Instruction drift compares repeated signatures of the same recorded instruction
artifact (base instructions, repository memory or a skill). Prompt snapshots,
ordinary files, reminders and tool listings do not count as instruction changes.
The comparison uses labels and character lengths, not instruction-body equality.
Only changes in newly completed turns may notify; historical findings are not
replayed when a log grows or monitoring resumes. To silence this rule immediately
in an older installed app, set **Instructions changed** to **Feed only**.

Two rules share one label on the wire: duplicate and low-information findings
both report as `contextWaste`, because they are the same complaint about the
same turn measured two ways.

Generic secret-like assignments produce warnings and skip common source-code,
path and placeholder matches. Provider-specific patterns retain critical
severity, but neither kind proves that a credential is live. Exports and
archives use broader redaction than the notification scan.

## Delivery is per rule, and has three settings

Each rule is `off`, `feed`, or `feedAndOs`. A rule set to `feed` writes to the
in-app drawer and never interrupts; `feedAndOs` also asks Windows for a toast.
Severity (`info`, `warning`, `critical`) describes the finding, not the
delivery -- a critical finding on a rule set to `off` is still not sent.

Subagent sessions are included or excluded globally rather than per rule,
because a subagent's context pressure is the parent's problem too.

## Why a toast is never assumed to have arrived

This is the part worth reading before trusting the feed's delivery column.

`tauri_plugin_notification` discards the result of the send, so every caller
learns the same thing whether the toast reached the shell or failed outright:
nothing. The desktop recorded 38 notifications as delivered on that basis --
a claim it had no evidence for. Delivery now calls the underlying Windows API
synchronously and reports what Windows actually said.

That is still not sufficient on its own. `ToastNotifier::Show` returns success
for an unregistered `System.AppUserModel.ID` and then silently drops the
toast: no error, no notification, nothing in the Action Center. So
deliverability is checked *first*, and a build Windows cannot attribute a
toast to fails before the API is called, naming the reason. What decides it is
whether a Start Menu shortcut carries the app id -- not where the executable
lives, so a development build delivers exactly as well as an installed one.

A finding therefore carries one of three outcomes, and the drawer shows which:
delivered, failed with a reason, or never requested because the rule was set
to `feed`.

## Following a finding

Selecting a finding opens Conversation in its session, regardless of the
currently selected tab. Findings that name a log line load pages until that
record is reached, expand it and highlight it. Findings that name only a turn
load and highlight the first conversation record for that turn. A finding
without either destination opens the session overview.

The record header stays in view after expansion and full-text loading. If a
named log line is absent from the conversation, the view says so rather than
silently landing on a different record. Selecting the same finding again
reopens and scrolls to its record.

The ring sits on the whole record rather than on the matched text. Secret
findings deliberately carry no offsets and no matched value, so the record is
the smallest unit the interface is allowed to point at.

New Windows toasts carry a `contexttrace://notification/<id>` destination.
Clicking restores the running window or launches the installed app, then follows
the same navigation as the in-app feed. Startup activations wait until the
frontend is ready. If the notification has expired from local history, the app
opens the feed with an explanation. The test notification opens the feed.

The protocol is registered by the updated Windows installer; an existing
installation must be upgraded to receive that registration. Portable and
development builds do not register a protocol at runtime. Toasts created by
older builds retain their original behavior.

## Where the feed lives

Findings persist to `state.json` under a `notifications` directory beside the
session archive -- on Windows, `%LOCALAPPDATA%ContextTrace-archive`. The feed
is history, not a live query: fixing a rule stops future findings but never
rewrites past ones. An alert that a later build would not raise stays in the
drawer until dismissed, which is the right behaviour for an audit trail and a
confusing one if you expect the list to re-evaluate itself. `ct secrets <id>`
is the live check; the drawer is the log.

## Known limits

**Nothing is sent while the app is closed.** The rules run in the desktop
process. A session that crosses a threshold overnight is found on next start,
and arrives marked as catch-up rather than as something that just happened.

Live session observation runs every 2.5 seconds independently of notification
preferences. Turning off notifications leaves Follow live and cache invalidation
active, without parsing sessions for notification rules. The observer tracks
agent-qualified descriptor fingerprints, coalesces changes, and detects
rotation/deletion on successful discovery. A failed agent discovery preserves
its previous inventory; other agents still refresh. Failed UI emissions retry.
Notification baselining and catch-up use separate persisted checkpoints.
