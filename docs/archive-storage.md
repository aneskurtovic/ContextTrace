# Archive storage and recovery

New archives keep each copied revision in sessions/<agent>/<id>.<sha256>.jsonl.
The append-only manifest.ndjson selects the latest acknowledged revision for
each agent and session ID. Old entries without versioned_copy continue to read
sessions/<agent>/<id>.jsonl; rearchiving keeps that legacy copy.

Writers coordinate through an OS-held writer.lock file. Its presence alone
does not mean a writer is active: closing the handle or exiting the process
releases the lock. Scratch files are exclusive and unique per ingest.

The copied bytes are flushed and synced before their manifest reference is
appended and synced. An incomplete manifest tail is preserved on a separate
line, allowing the next complete entry to load normally. A failed manifest
commit leaves previous referenced copies unchanged.

Unreferenced revisions are retained as recovery evidence. Retrying ingestion
of the same bytes reuses an intact digest-named revision. These files are not
automatically listed as acknowledged sessions or deleted. General orphan
reconciliation is not yet implemented; preserve the archive directory when
recovering metadata.

File syncs and, on Unix, directory syncs are exercised by local regressions.
They do not establish power-loss durability on every filesystem or clean-host
installer/updater acceptance. Older application versions do not understand
the new revision layout.
