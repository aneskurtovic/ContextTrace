# Projects and session resume

ContextTrace derives projects from the working folders recorded in agent logs.
They are not necessarily Git repositories. Temporary workspaces are sessions
whose recorded folders are under known OS temporary locations or the current
system temporary directory. A repository merely containing `tmp` in its name
does not qualify.

The project dropdown folds temporary folders into **Temporary workspaces** by
default. Select that entry to browse their sessions together, or **Expand
temporary folders** to select an individual folder. The group's count uses the
same agent, search and subagent filters as the session list.

**Manage project visibility** offers **Default**, **Hide from project list**
and **Always show** for each recorded path. Always show keeps a temporary folder
in the main dropdown. Preferences are saved locally in the desktop webview;
they only change the dropdown, not session discovery, search or dashboard
totals. A selected hidden folder stays explicit until you change the filter.
Saved preferences outside current search results can also be reset there.

**Folder missing** is separate from temporary-folder classification. Logs
remain inspectable when their working folder has been deleted or moved.
Unrecorded or unresolved legacy folder names are not labelled missing.

## Resume in the original agent

Each session row has a **Resume** action. Review its working folder and native
PowerShell command, then choose **Open terminal**, or **Copy command** to run it
yourself. ContextTrace uses the original agent's CLI:

```powershell
codex resume <session-id> --cd <working-folder>
claude --resume <session-id>
```

Both launch from the selected working folder. For a missing or unrecorded
folder, enter an existing absolute local folder first. Codex's explicit `--cd`
override also prevents it from choosing a deleted saved folder. Claude session
lookup across projects depends on the installed CLI version. A subagent row
offers **Resume parent** and follows recorded parent references to the main
session; an unavailable parent produces an explanation rather than selecting
another conversation.

The CLI must be locally installed. ContextTrace checks PATH and the standard
native install locations, and rechecks the session, folder and CLI when you
launch. Original agent session logs are required: an archive-only copy cannot
be silently restored into another application's session storage. Browser demo
mode cannot launch local agents.

The terminal is interactive. ContextTrace does not send a prompt automatically,
change agent approval settings, switch Git branches, or wait for proof that the
agent finished restoring the conversation. The CLI may connect to its service
and continues with its configured permissions. A terminal-opened message means
the process was started, not that a model request succeeded.

## Verification limits

Automated checks cover project grouping/visibility, dialog behavior, parent and
UUID validation, missing folders, and PowerShell argument handling using a stub
CLI on Windows. Local CLI help was checked. These checks do not establish that
a real Codex or Claude CLI restored a conversation, that a visible terminal
behaved correctly in the installed app, or that every agent version can resume.
Native desktop visual QA and real-agent end-to-end resume remain unverified.

## Saved context and summaries

Native resume continues the same saved conversation, including the compaction
state stored by its agent. Both CLIs support `/compact` inside a conversation to
summarize context. This is different from starting a fresh conversation using a
user-supplied summary; ContextTrace's Resume action does not generate or inject
such a summary.

Commands were checked against local CLI help and the official
[Codex command reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli#codex-resume),
[Claude CLI reference](https://code.claude.com/docs/en/cli-reference) and
[Claude session/context guide](https://code.claude.com/docs/en/how-claude-code-works).
