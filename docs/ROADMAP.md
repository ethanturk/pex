# Pex Roadmap — Making Code Review Easy

> Long-horizon plan for evolving Pex from "AI-assisted PR review" into a
> review system reviewers actually trust. Owned vision; living document.

## Why this exists

Code review was never easy. AI-generated code makes it harder: volume is up,
each change needs more oversight, and the human review budget is flat. Pex
already has the machinery — a multi-pass engine, specialist reviewers,
line-anchored findings posted to Azure DevOps. The next level is not *more
machinery*. It is **trust**.

The four goals driving this roadmap:

1. **Pull forward high-priority fixes** — the one real bug surfaces first.
2. **Push back low-priority fixes** — nits never bury the signal.
3. **Use AI for REAL, high-quality reviews** — precision over volume.
4. **Automatic, trustworthy feedback** — it runs itself, and you believe it.

## The thesis: trust is the bottleneck, not capability

A review tool is used heavily only when the reviewer believes it. The first
false "bug," the first real issue buried under ten nits, and people stop
reading — at which point the tool *adds* oversight cost instead of removing
it. The product we are really building is **precision you can prove**. Every
phase below serves that.

---

## Foundation: the PR Review Toolkit

Pex's Thorough mode is distilled from Anthropic's
[`pr-review-toolkit`](https://github.com/anthropics/claude-code/tree/main/plugins/pr-review-toolkit).
We continue to build on it, but adapt its model deliberately:

| | PR Review Toolkit | Pex |
|---|---|---|
| **When** | Author-side, pre-PR | Reviewer-side, post-PR |
| **Trigger** | Natural-language ("check the tests") | Orchestrated, deterministic pipeline |
| **Unit** | Full `git diff` + repo context | Per-hunk (today) → file/PR context (goal) |
| **Output** | Markdown report to the author | Line-anchored ADO threads + summary |

### What we adopt from the toolkit

- **The 0–100 confidence scale with a reporting threshold.** The toolkit's
  `code-reviewer` only surfaces issues scoring **≥80**, split into Critical
  (90–100) and Important (80–89). Everything below 80 is treated as a likely
  false positive or a low-impact nit and is *not reported*. This is exactly
  the precision gate and noise floor goals 1–3 require. **Pex will adopt this
  confidence model as its foundation.**
- **Precision over volume.** Every agent filters aggressively. We make that a
  structural property, not a prompt aspiration.
- **The six specialists.** Pex ships five of them
  (`src-tauri/src/ai/prompts.rs`): `code-reviewer`, `silent-failure-hunter`,
  `comment-analyzer`, `pr-test-analyzer`, `type-design-analyzer`. The toolkit's
  sixth — **`code-simplifier`** — is missing and should be added (it maps
  directly to the existing `/simplify` skill's intent).

### The confidence model

Today `Finding` (`src-tauri/src/review/engine.rs`) carries only a 3-level
`Severity` (Critical / Moderate / Minor). We extend it:

- **Severity** answers *how bad if true* (impact).
- **Confidence (0–100)** answers *how sure we are* (likelihood).

Triage needs both. A 95-confidence style nit is not blocking; a 60-confidence
"possible data loss" is not postable as fact. The reporting threshold (default
**≥80**, configurable) becomes the single dial that tunes Pex's noise floor —
and, in Phase 3, the dial we tune *with evidence*.

---

## Phase 1 — Make findings REAL (precision foundation) ✅ shipped

Implemented — see [`design/phase-1-precision.md`](./design/phase-1-precision.md).

The prerequisite for everything else. Two structural weaknesses cap quality
today.

### 1.1 Kill hunk isolation (the #1 false-positive source)

`review_single_hunk` reviews each hunk alone. The model cannot see that a
"missing import" exists two hunks up, or that an "unhandled error" is caught by
the caller. These produce confident, wrong findings — the fastest way to lose
trust.

- **Near term:** give each reviewer the full surrounding file. `FileInput`
  already holds complete `old_content` / `new_content` — pass them as context,
  with the hunk marked as the region under review.
- **Longer term:** a lightweight symbol/definition index so cross-file claims
  ("this function doesn't exist") can be checked before they surface.

### 1.2 Add an adjudicator / verification pass

Every candidate finding flows straight to file-aggregate today. Insert a gate
between specialist output and aggregation:

- Each candidate must produce **claim → evidence (cited lines) → verdict**.
- Findings whose cited evidence does not check out against the actual file are
  **dropped** before the reviewer ever sees them.

This single step does more for trust than any new specialist.

### 1.3 Confidence as a first-class field

- Extend `Finding` and `FileAggregateFinding` with `confidence: u8` (0–100).
- Each specialist emits a confidence score (mirroring the toolkit's rubric).
- Filter at the reporting threshold (default 80) before posting.

### 1.4 Add the sixth specialist: `code-simplifier`

Complete the toolkit set. Add `ReviewCodeSimplifierSystem` to `PromptKey` and
`THOROUGH_SPECIALISTS`. Quality-only lens (reuse, redundancy, clarity); it must
respect the same confidence contract so its output cannot become nit-noise.

### 1.5 The evaluation harness (built alongside 1.1–1.4)

**You cannot improve precision you don't measure.** Build a golden set of
~20–30 real PRs with labeled findings — true positives *and* known false
positives. Re-run the engine against it on every prompt/model change and track
precision / recall. Without this, every "improvement" is a guess and trust
erodes silently. Start thin; grow it as real reviews surface new cases.

**Phase 1 exit criteria:** measured false-positive rate down, confidence
scores present on every finding, threshold filtering live, harness green.

---

## Phase 2 — Pull forward / push back (triage) ✅ shipped

Implemented — see [`design/phase-2-triage.md`](./design/phase-2-triage.md).

Findings are ranked on `severity × confidence × blast-radius` into tiers:

- **Blocking** — high-severity, high-confidence. *Pulled forward*: top of the
  summary, posted as active threads, optionally auto-set reviewer vote to
  "wait for author."
- **Should-fix** — real but not gating.
- **Nits / FYI** — *pushed back*: collapsed into a single rollup comment or
  suppressed below threshold. Never individual blocking noise. This is the
  lever that makes review feel *easy*.

The data already exists (`Severity` + new `confidence`); this phase adds
ranking and the rollup presentation.

---

## Phase 3 — Make it TRUSTWORTHY (the feedback loop) ✅ shipped

Implemented — see [`design/phase-3-trust-loop.md`](./design/phase-3-trust-loop.md).

What compounds. Today nothing learns when a reviewer dismisses a finding.

- **Capture actions** per finding: accepted / dismissed / edited. The frontend
  already supports selectable findings posting — extend it to record verdicts.
- **Suppression memory:** fingerprint dismissed findings so they don't
  re-surface on the next iteration (the engine already has iteration support
  and resumable SQLite state to hang this on).
- **Calibration view:** precision per specialist, accept-rate by severity. Now
  the noise-floor threshold is tuned *with evidence*, and the tool can *prove*
  it is improving.
- **Incremental review:** review only the delta between iterations, not the
  whole PR again.

---

## Phase 4 — Make it AUTOMATIC (earned autonomy)

Safe only *after* Phase 3 proves precision. Trust tiers govern autonomy:

- Auto-trigger review on PR open / new iteration, in the background.
- Auto-post only the highest-confidence Blocking findings; everything else
  queues for the human gate.
- Longer horizon: a headless / CI mode so reviews run server-side, not only in
  the desktop app — where "scaling" actually lands.

---

## Sequencing

```
Phase 1  Precision foundation  ──┐  (+ eval harness, built in parallel)
Phase 2  Triage                  │  depends on confidence from P1
Phase 3  Trust feedback loop     │  needs findings from P1/P2 to act on
Phase 4  Automation              ┘  only safe once P3 proves precision
```

Each phase is shippable on its own and de-risks the next. The eval harness
underpins all of them: it is how we know each phase actually worked.

## Guiding principles

- **Precision over volume**, always. A missed nit costs nothing; a false
  "bug" costs trust.
- **Never post as fact what we can't cite.** Evidence or it doesn't ship.
- **Tune with data, not vibes.** The harness and calibration view are the
  source of truth.
- **The reviewer is in control.** Automation earns autonomy by proving
  precision first — never before.
