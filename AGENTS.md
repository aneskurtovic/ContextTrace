# Repository agent instructions

## CI and release policy

Always use Woodpecker CI for continuous integration and release packaging. Never
add or run GitHub Actions jobs, use GitHub-hosted runners, or dispatch the
repository's manual GitHub release workflow. Keep validation and Windows
release builds on the configured Woodpecker runners.

Follow the project verification and release instructions in `CLAUDE.md` and
`docs/RELEASING.md`.
