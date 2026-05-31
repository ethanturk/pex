# Phase 3 Design — The Trust Feedback Loop

> Make it trustworthy: capture what the reviewer does with findings, stop
> re-surfacing dismissed ones, show calibration so the noise floor is tuned with
> evidence, and review incrementally. Builds on Phase 1 (confidence) and Phase 2
> (tiers). See [`../ROADMAP.md`](../ROADMAP.md).
>
> **Status: implemented.** Verdict capture, suppression memory, calibration
> metrics + UI, and incremental review. Backed by unit tests
> (`review::feedback`).

## Goal & exit criteria

Phases 1–2 made findings real and ordered them. But nothing learned from the
reviewer: a dismissed nit came back every run, and there was no evidence to tune
thresholds. Phase 3 closes the loop:

- **Capture verdicts** — every finding the reviewer acts on is recorded as
  accepted / dismissed / edited.
- **Suppression memory** — dismissed findings don't re-surface on the next run.
- **Calibration** — accept rates by severity and tier, so thresholds are tuned
  with data, not vibes.
- **Incremental review** — re-reviews can cover only what changed since the last
  reviewed iteration.

## Fingerprinting (the hard part)

Suppression and calibration both need to recognize "the same finding" across
runs, but LLM findings drift: wording changes between runs, line numbers change
between iterations. So we fingerprint on **file path + normalized comment**:

- `normalize_comment` lowercases, strips digits and punctuation, and collapses
  whitespace — so "Null deref at line 12: `foo`…" and "null deref at line 5 -
  foo…" hash identically.
- `fingerprint = FNV-1a(file_path \0 normalized_comment)`, hex-encoded. FNV-1a
  (not `DefaultHasher`) because the value is **persisted and compared across
  runs**, so it must be deterministic forever.

This is robust to line/word drift, specific enough not to merge distinct issues
in a file. (A degenerate comment of only digits/punctuation normalizes to empty;
real findings always carry prose, so this doesn't arise in practice.)

## Data model

One table (`finding_verdicts`), keyed `(pr_key, fingerprint)` so the latest
verdict per finding wins:

```
pr_key, fingerprint, verdict, file_path, severity, tier, confidence, comment, updated_at
```

`pr_key` is built identically on every path — `org_url/project/repo/pr_id` —
so the verdict commands and the engine's suppression lookup line up.

## 3.1 Capture verdicts

A `Verdict` is `accepted` / `dismissed` / `edited`. It's recorded via the
`record_finding_verdict` command, which derives `pr_key` server-side and
fingerprints the finding. The sidebar wires it up:

- **Accepted** — posting a finding unedited (bulk "Post" or the editor with no
  text change).
- **Edited** — posting from the editor after changing the wording.
- **Dismissed** — the new **Dismiss** button on each finding row. The row greys
  out and shows "Dismissed ✓ (suppressed next run)".

All verdict writes are best-effort: a failure never blocks posting.

## 3.2 Suppression memory

At the end of `run_review`, after findings are tiered and sorted, the engine
loads the dismissed fingerprints for this `pr_key` and drops any finding whose
fingerprint matches. The suppressed count is logged and included in the `done`
progress event. Only **dismissed** findings are suppressed — accepted/edited
ones are real and may legitimately recur.

## 3.3 Calibration

`get_review_calibration` aggregates every recorded verdict into:

- totals (accepted / dismissed / edited) and an overall accept rate
  (`(accepted + edited) / total`),
- per-**severity** buckets (critical / moderate / minor),
- per-**tier** buckets (blocking / should-fix / nit / fyi),

each with its own accept rate. A **Calibration** tab in AI settings renders this
with a Refresh and a Reset (clears all verdicts → also clears suppression). The
point: if "minor" or "nit" is mostly dismissed, that's evidence to raise the
confidence threshold or the critical line — tuning with data instead of vibes.

> Per-*specialist* precision also ships, via a `sources` field on each finding:
> the adjudicator echoes the `[specialist-label]` tags of the candidates it
> merged, those are validated against the known specialist set, persisted on the
> verdict, and aggregated into a "By specialist" calibration bucket (a merged
> finding credits each contributing specialist). This attribution is
> LLM-reported, so approximate; the plan to replace it with deterministic
> Rust-side matching is filed in
> [`future-deterministic-attribution.md`](./future-deterministic-attribution.md).

## 3.4 Incremental review

Opt-in (`ai_incremental_review`, default off). The last reviewed iteration is
stored per PR (`review_last_iteration:{pr_key}`). On a re-review with a prior
iteration recorded, the engine narrows the file set to paths changed across
`(last, current]` — computed by unioning ADO iteration changes
(`changed_paths_since_iteration`). It falls back to a **full review** whenever
incremental can't safely apply (disabled, no prior iteration, no forward delta,
or the delta doesn't intersect the current file list), so it never silently
skips everything. The reviewed iteration is recorded after each successful run
(both the preview and the post-to-ADO paths).

## What this phase deliberately does NOT do

- No per-specialist precision (attribution is lost at adjudication).
- Suppression is scoped per PR, not global — a pattern dismissed on one PR can
  still surface on another (that's usually correct; cross-PR learning is a
  bigger, riskier step).
- Thresholds are not auto-tuned from calibration — the data is surfaced for the
  human to act on. Closing that loop automatically is future work (and pairs
  naturally with Phase 4's automation).

## Testing

- `normalize_comment` ignores line numbers / case / punctuation.
- `fingerprint` is stable across drift and path-specific.
- Dismissed findings are remembered per PR; latest verdict wins.
- Calibration counts and rates; last-iteration round-trips.
- Frontend typechecks; production build passes.
