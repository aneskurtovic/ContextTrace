# Cache and context usage

Context size includes fresh input, cache reads and cache writes. A cache read
reduces the applicable API token price; it still occupies model context.
Overview shows the session peak, and Context shows the selected turn's used
tokens, recorded window and percentage. A missing window stays unknown; a
published maximum for a model is not proof of a session's configured limit.

Spend shows recorded fresh input, cache reads, cache writes and cache read share
independently of whether prices are available. Unknown buckets are shown as
not reported. The share is reads / (fresh input + reads + writes), calculated
only for turns with complete input splits; coverage is shown alongside it.
These are token shares, not cache lookup hit/miss counts. Fresh input can be
new or uncacheable content, and a write does not identify a cache lookup miss.

Codex reports cached input as a subset of total input. The adapter subtracts
cached input before storing fresh input so context totals stay unchanged and
cached tokens receive the catalog's cache-read rate. Persisted Codex logs do
not supply a separate cache-write bucket in the supported contract. Absent or
inconsistent cache splits remain unknown and are excluded from spend.

Claude reports fresh input, cache reads and cache creation separately. For
multi-iteration messages, reconstruction retains the largest request's input
to describe its context. Those samples are labelled and excluded from spend:
they cannot establish billable input across every call. All-zero placeholders
do not establish measured context or a cache-read share.

Prices remain standard text-token API list-price estimates, with per-category
cache rates from the recorded LiteLLM revision. They are not subscription bills
and do not establish nondefault cache durations, tool/media charges or negotiated
rates. Forecasts require complete supported usage and pricing.

Spend requires recorded input and output plus all applicable cache buckets,
including when a count is zero. An absent count stays unknown. Incomplete
turns are unpriced and disable forecasts; scenario caps do not fill missing
counts. Absent Codex cache writes remain inapplicable in the supported contract.

Provider references: [OpenAI prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching)
and [Claude prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching).
