# Local files in context composition

Expanding a context category shows the items that contributed to it. When an
item records a filesystem target that exists locally, its label opens the file
with Windows' default application. Directories open in Explorer. Each file row
also offers **Show in Explorer** and **Copy path**.

The exact path is retained separately from the shortened display label. Long
paths and repeated spaces in filenames are preserved. Tool results inherit the
target from their matching call. Commands, queries, URLs and ordinary message
text do not become file links.

Relative tool paths resolve only when the call records a working directory.
The application never resolves them against its own directory. Paths from
another operating system, UNC/device paths and unresolved relative paths remain
unavailable locally. Missing temporary files say **File no longer available**;
their path can still be copied. Executables and launch shortcuts offer Explorer
rather than direct execution.

Opening checks the filesystem again, so deletion after a row was rendered
produces an error in that row. A browser preview cannot open local files.

Opening a file shows its current bytes on disk. It does not recreate the file
as it existed when the agent originally read it, and archiving a session does
not archive the files mentioned in it.
