# Phase 2 Design — Triage & Prioritization

> Pull high-priority fixes forward; push low-priority ones back. Builds on the
> `confidence` field established in Phase 1. See [`../ROADMAP.md`](../ROADMAP.md)
> for the long-horizon arc and [`phase-1-precision.md`](./phase-1-precision.md)
> for the precision foundation.
>
> **Status: implemented.** Tier model, strict triage ordering, pull-forward /
> push-back posting (rollup), tier-grouped sidebar with collapsed low-priority
> sections and default-selected actionable findings, and opt-in auto-vote on
> blocking. Backed by unit tests (`review::engine`).

## Goal & exit criteria

Phase 1 made findings real and scored them. They still arrived as a flat list
grouped only by severity, so the one blocking bug sat next to ten nits. Phase 2
imposes a strict triage order and acts on it:

- Every finding is assigned a **tier**: Blocking / Should-fix / Nit / FYI.
- Findings are ordered Blocking-first everywhere (sidebar, posting, stats).
- **Pull forward:** Blocking + Should-fix are surfaced prominently, pre-selected
  for posting, and posted as individual comments.
- **Push back:** Nit + FYI are collapsed in the UI and posted as a single
  rollup comment, never as individual noise.
- Optional: posting a review with a Blocking finding can cast a "wait for
  author" vote.

## The tier model

Tiers are derived **deterministically** from the two Phase 1 axes plus a blast-
radius proxy (line-anchored vs. file-level), so the same finding always lands in
the same tier — predictability is what makes triage trustworthy.

`tier_for(severity, confidence, line_start)` (`src-tauri/src/review/engine.rs`):

| Condition | Tier |
|-----------|------|
| Critical, confidence ≥ 85 | **Blocking** |
| Critical, confidence < 85 | **Should-fix** |
| Non-critical, no line anchor | **FYI** |
| Moderate, confidence ≥ 80 | **Should-fix** |
| otherwise (Minor, or Moderate < 80) | **Nit** |

Critical findings are always actionable (never demoted to a nit or FYI), even
without a line anchor — a critical architectural issue matters regardless of
whether it points at one line. Non-critical findings with no line to act on are
informational (FYI).

`Tier` carries `rank()` (Blocking = 0 … FYI = 3), `is_actionable()` (Blocking /
Should-fix), and `label()`. It is stored on each `Finding` (serialized to the
frontend) and computed once when findings are flattened during synthesis.

## Ordering

After flattening, findings sort by `tier.rank()`, then **confidence
descending**, then file path, then line. Blocking-first ordering flows to:

- the sidebar (grouped by tier), and
- `post_findings` (which iterates the already-sorted list), and
- the Statistics block, which now includes a triage line
  (`N blocking, N should-fix, N nit, N FYI`).

## Pull forward / push back

### Posting (`post_findings`)

The full auto-post path splits findings by `tier.is_actionable()`:

- **Actionable** (Blocking, Should-fix) → one ADO thread each, prefixed with a
  tier tag (`🔴 BLOCKING —`, `🟡 SHOULD FIX —`), anchored to the line when known.
- **Pushed back** (Nit, FYI) → a single rollup comment
  (`build_rollup_comment`) listing each as `**path:line** (Tier) — comment`, so
  they live in one place instead of cluttering the diff with low-value threads.

### Sidebar (`PRReviewSidebar.tsx`)

- Findings are grouped by tier (Blocking → FYI) instead of severity.
- **Push back:** Nit and FYI sections render collapsed (`▸ show`) so they never
  bury the actionable findings; Blocking and Should-fix render expanded.
- **Pull forward:** when a run's output arrives, actionable findings are
  **pre-selected** for posting and low-priority ones are left unselected, so the
  default "Post N findings to ADO" action ships the blockers first.
- The Statistics panel shows the triage breakdown alongside severity counts.

## Optional: auto-vote on blocking

Strictly opt-in (`ai_auto_vote_on_blocking`, default off; surfaced as a checkbox
in AI settings). When **posting a review to ADO** (`start_review_post`) and the
result has ≥ 1 Blocking finding, Pex casts a "wait for author" vote
(`VOTE_WAIT_FOR_AUTHOR = -5`) via the existing reviewer-status endpoint, so a PR
can't be approved out from under an unaddressed blocker. Off by default because
auto-voting is a visible side effect.

## What this phase deliberately does NOT do

- No learning from accept/dismiss yet — that is Phase 3 (the trust loop). The
  tiers here are computed fresh each run, not tuned by feedback.
- Thresholds for the tier cutoffs (85 / 80) are constants, not user settings,
  to avoid sprawl; they can be promoted later if calibration data justifies it.
- The final-synthesis prose is unchanged; triage acts on the structured
  findings, which is where posting and the sidebar read from.

## Testing

- `tier_for` across critical/moderate/minor × confidence × anchor.
- `Tier::rank` ordering and `is_actionable`.
- `build_rollup_comment` lists each pushed-back finding with its tier.
- Frontend typechecks; production build passes.
