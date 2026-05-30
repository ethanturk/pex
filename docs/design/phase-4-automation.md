# Phase 4 Design — Automation (Earned Autonomy)

> Make it automatic: auto-review new iterations, auto-post only the highest-
> confidence blockers, and run reviews headlessly in CI. Safe only because
> Phases 1–3 made findings precise and gave us calibration to prove it. See
> [`../ROADMAP.md`](../ROADMAP.md).
>
> **Status: implemented.** Auto-review orchestration, auto-post of high-
> confidence blocking findings, and a headless CI binary. Backed by unit tests
> (`review::engine`).

## Goal & exit criteria

The first three phases produced trustworthy, triaged findings and a feedback
loop. Phase 4 spends that trust on autonomy — but *graduated* autonomy, gated so
the tool never does something the reviewer wouldn't:

- **Auto-review** — review a PR automatically when it has a new iteration.
- **Auto-post** — post only the highest-confidence Blocking findings unattended;
  everything else waits for the human gate.
- **Headless / CI** — run the same review server-side and gate a pipeline on it.

Every piece is **opt-in and off by default.** Autonomy is earned, not assumed.

## The autonomy ladder

Three independent toggles, each a bigger grant of trust than the last:

| Setting | Default | Grant |
|---|---|---|
| `ai_auto_review` | off | run reviews without being asked |
| `ai_auto_post_blocking` | off | post comments without a human |
| `ai_auto_post_confidence` | 90 | how sure a blocker must be to post |

A team can enable auto-review alone (reviews are ready when you open a PR, but
nothing is posted), then add auto-post once calibration (Phase 3) shows the
blocking tier is trustworthy.

## Auto-review

`PullRequest.iterationCount` is the trigger signal. The last reviewed iteration
is already tracked per PR (Phase 3, `review_last_iteration:{pr_key}`).

- `should_auto_review(enabled, last, current) = enabled && current > last`
  (a never-reviewed PR has `last = None → 0`, so it qualifies).
- `auto_review_candidates` (command) takes the listed PRs with their iteration
  counts and returns the IDs that qualify — empty when auto-review is off.
- The frontend orchestrator (`autoReview.ts`) is invoked on every PR-list
  load/refresh. It enqueues candidates and **drains the queue one at a time**,
  backing off whenever any review (user- or auto-initiated) is in flight — the
  Rust engine already serializes runs, and we serialize the queue so we never
  stampede the provider. Auto-reviews run in **Fast** mode.

A per-PR badge in the list shows review status (reviewing / N findings / N
blocking / posted / failed), so automation is visible, not silent.

## Auto-post

After an auto-review completes, the orchestrator calls
`auto_post_review_findings` with the result. The backend:

- `select_auto_post_findings(findings, floor)` keeps only **Blocking** tier at or
  above the confidence floor (default 90), in the engine's existing
  blocking-first order.
- Posts each via the shared `post_single_finding` (same tier-tagged formatting
  as a manual post) and records an **accepted** verdict (so calibration sees it
  and it isn't re-flagged next run).
- Returns the count posted; a no-op (returns 0) unless auto-post is enabled.

Everything not auto-posted stays in the sidebar for the human gate — the point
is to clear the unambiguous blockers automatically, not to replace review.

## Headless / CI

`examples/review_cli.rs` runs a review server-side with no desktop app, reusing
the same engine building blocks (`AdoClient`, `review_single_file`, `tier_for`):

- Auth and coordinates come from env / a PR-URL argument (`PEX_ADO_PAT`,
  `PEX_AI_*`, and either a PR URL or `PEX_ORG_URL`/`PEX_ADO_PROJECT`/
  `PEX_ADO_REPO`/`PEX_ADO_PR`) — CI-friendly.
- It reviews each changed file headlessly, tiers the findings, prints them
  grouped by tier, and **exits non-zero if there are Blocking findings**, so it
  can gate a pipeline. Tuning (mode, thresholds) is via env.

This is where "scaling" lands: reviews run wherever CI runs, not only on a
reviewer's machine. (A full long-running service — webhooks, persistence,
multi-tenant — is the next step beyond this; the CLI is the minimal real
server-side capability.)

## What this phase deliberately does NOT do

- No auto-posting of non-blocking findings, ever — only the top tier, only above
  a high floor.
- Auto-review uses Fast mode and serializes — no parallel fan-out that could
  exhaust a provider.
- The CI binary is a one-shot reviewer, not a daemon; it doesn't listen for
  webhooks or persist state.
- Thresholds still aren't auto-tuned from calibration — that remains a human
  decision (surfaced by Phase 3).

## Testing

- `should_auto_review` across enabled/disabled and iteration relationships.
- `select_auto_post_findings` keeps only high-confidence blocking, non-empty.
- Frontend typechecks; production build passes; `review_cli` compiles.
