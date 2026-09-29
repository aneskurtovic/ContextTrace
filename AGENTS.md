# Repository agent instructions

## CI and release policy

Always use Woodpecker CI for continuous integration and release packaging. Never
add or run GitHub Actions jobs, use GitHub-hosted runners, or dispatch the
repository's manual GitHub release workflow. Keep validation and Windows
release builds on the configured Woodpecker runners.
Run version-tag packaging, publishing, and published-asset verification in the
Woodpecker Windows release pipeline. Local checks may help development, but do
not use an interactive workstation run as release evidence. Record separate
clean-host installer and updater acceptance distinctly from CI results.

Follow the project verification and release instructions in `CLAUDE.md` and
`docs/RELEASING.md`.
