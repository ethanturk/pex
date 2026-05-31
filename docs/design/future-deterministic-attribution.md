# Future Design — Deterministic Specialist Attribution

> **Status: filed for the future, not implemented.** The shipped per-specialist
> precision uses adjudicator-reported `sources` (the `sources` field on findings,
> populated by the file adjudicator). That is *approximate* — the LLM tells us
> which specialists raised a finding. This document is the plan for replacing it
> with attribution computed deterministically in Rust, if the approximate
> numbers ever prove too noisy to act on.

## Why we'd want this

The `sources` approach trusts the adjudicator LLM to echo the
`[specialist-label]` tags it merged. Fine for *aggregate* tuning
("silent-failure-hunter trends noisy"), but it has two failure modes:

1. **Misattribution** — the model credits the wrong specialist, or omits one it
   actually merged.
2. **Drift** — a prompt or model change silently changes attribution quality and
   nothing catches it.

Deterministic attribution removes the LLM from the attribution path, so
per-specialist precision becomes a fact about the pipeline, not a model opinion.

## The core idea

Attribution is destroyed at adjudication today (`FILE_AGGREGATE_SYSTEM` merges
tagged prose candidates into structured findings and drops the tags). The fix is
to **retain the per-hunk specialist candidates and match each final finding back
to them in Rust**, after adjudication.

```
specialist passes ──► tagged candidates (retained) ─┐
                                                     ├─► adjudicator ─► final findings
                                                     │                       │
                                                     └──── deterministic ◄────┘
                                                          back-matching
                                                       (line overlap + text sim)
```

## Implementation sketch

### 1. Retain structured per-hunk candidates

`review_single_hunk` currently joins the Thorough specialists' output into one
prose string. Keep them structured instead:

```rust
pub struct HunkCandidate {
    pub specialist: String,            // PromptKey::specialist_label()
    pub hunk_index: usize,
    pub new_line_hint: Option<usize>,  // best-effort cited NEW-side line
    pub text: String,
}
```

The engine already runs each specialist separately and tags its output — this
just stops flattening to a string. Parse a line hint from the prose where
present (specialists are told to "reference exact NEW-side line numbers"), else
`None`. Thread the candidates to the file-aggregate step alongside the prose the
adjudicator still consumes (adjudicator input is unchanged; we just *also* keep
the structured candidates).

### 2. Match final findings back to candidates

After `parse_file_aggregate` produces `FileAggregateFinding`s:

```rust
fn attribute(finding: &FileAggregateFinding, candidates: &[HunkCandidate]) -> Vec<String>
```

Score each candidate, keep specialists above a floor:

- **Line proximity** — candidate `new_line_hint` within ±N lines of the
  finding's `line_start` (file-level findings skip this signal).
- **Text similarity** — token/Jaccard or normalized Levenshtein between candidate
  text and the finding comment, reusing `feedback::normalize_comment`. The
  `similar` crate (already a dependency) supplies the ratio — no new crates.
- Combine (e.g. `0.6 * line + 0.4 * text`); attribute every specialist whose best
  candidate clears the floor. Empty → the same `unattributed` bucket the
  approximate version uses.

### 3. Everything downstream already exists

The `sources` field on `Finding`/`FileAggregateFinding`, the `sources` column on
`finding_verdicts`, and the `bySpecialist` calibration bucket all shipped with
the approximate version. Deterministic attribution only changes *how `sources` is
computed* — write the matched labels instead of the adjudicator-reported ones. No
schema, command, or UI changes needed to switch.

## Cost / trade-offs

- **More plumbing**: candidates must survive from the per-hunk pass to
  post-adjudication (today discarded after the prose join).
- **Tuning**: the match threshold/weights need their own calibration — too loose
  over-credits, too tight under-credits.
- **Merged findings still credit multiple specialists** — same as today, and
  correct.

## Decision rule

Stay on adjudicator-reported `sources` until the Calibration tab's per-specialist
numbers are actually being used to change prompts *and* someone distrusts them.
The switch is contained (only how `sources` is computed), so deferring costs
nothing — this doc exists so the path is ready when the need is real.
