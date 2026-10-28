# Comparing recorded instruction files

Desktop, CLI and MCP comparisons use the same injected local-file reader.
Relative attachment labels require a recorded local absolute project directory;
absolute repository and home-directory files remain supported. UNC, device,
drive-relative, alternate-stream and reserved Windows device names are refused
before reading. On Windows, mapped network drives and unknown drive categories
are refused before traversing the attachment path.

The reader rejects symlinks and Windows reparse points, including parent
junctions, checks the canonical target, and reads only regular files. This means
instructions beneath a linked directory require an ordinary local path.
Bodies over 4 MiB are refused; a bounded read also catches growth after the
metadata size check. Refusals report unsafePath, notRegular or tooLarge and
carry no current digest. Missing and unreadable files remain separate states.
No refusal establishes an instruction change.

These checks prevent direct evidence-driven network/device paths and ordinary
link traversal. They do not establish a race-proof filesystem sandbox against a
process concurrently replacing ancestor directories. Unix mount locality is
not determined by this reader. Native network authentication, special filesystem
behavior and operating-system file associations have not been exercised.
