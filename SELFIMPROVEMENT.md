# Self-Improvement Loop

How to turn the diagnostic traces (the *features*: what the engine and model
did) plus the recorded verdicts (the *labels*: what the reviewer thought of it)
into concrete, evidence-based improvements to both the **deterministic** behaviors
(confidence threshold, critical line, anchor check, suppression) and the
**non-deterministic** ones (the adjudicator and specialist prompts).

This is the loop the rest of the system was built to enable:
[`docs/design/diagnostics-trace.md`](docs/design/diagnostics-trace.md) defines
the trace; [`docs/design/phase-3-trust-loop.md`](docs/design/phase-3-trust-loop.md)
defines the verdicts; [`docs/design/phase-1-precision.md`](docs/design/phase-1-precision.md)
defines the eval harness that gates every change.

> **Prime directive:** this process *recommends*; a human approves. No prompt or
> threshold change ships without the eval harness showing precision up and recall
> not down. Autonomy is earned — the same rule the product follows.

---

## 1. Inputs

| Source | What it is | Location |
|--------|-----------|----------|
| Traces | one JSONL file per run (events below) | `<app-data>/diagnostics/*.jsonl` |
| Verdicts | `finding_verdicts` rows | the app's SQLite DB (`pex.db`) |
| Fixtures | the regression gate | `src-tauri/tests/eval/fixtures/` |

Relevant event kinds (see the trace doc for full fields):
`run_start` (carries `prKey` + `settings`), `llm_call` (prompt+response per
stage), `hunk_candidate`, `adjudicated_finding`, `guard_drop` (+`reason`),
`suppressed`, `finding_final` (carries `fingerprint`), `run_done`.

Verdict columns: `pr_key, fingerprint, verdict ∈ {accepted,dismissed,edited},
severity, tier, confidence, sources, comment`.

---

## 2. The join key — fingerprint (must match the engine exactly)

The join between a trace event and a verdict is `(prKey, fingerprint)`.
`finding_final` and `suppressed` events carry `fingerprint`; `adjudicated_finding`
and `guard_drop` do **not** — recompute it from `filePath + comment` using the
*same* algorithm the engine uses (`review::feedback::fingerprint`), or the join
silently misses. Reference implementation (byte-for-byte equivalent to the Rust):

```python
def is_ascii_alpha(c):  return ('a' <= c <= 'z') or ('A' <= c <= 'Z')
def is_ascii_digit(c):  return '0' <= c <= '9'
def is_ascii_punct(c):
    o = ord(c)
    return 0x21<=o<=0x2F or 0x3A<=o<=0x40 or 0x5B<=o<=0x60 or 0x7B<=o<=0x7E

def normalize_comment(s: str) -> str:
    out, prev_space = [], False
    for ch in s:
        if is_ascii_alpha(ch):
            out.append(ch.lower()); prev_space = False
        elif ch.isspace() or is_ascii_digit(ch) or is_ascii_punct(ch):
            if not prev_space and out:           # collapse runs of non-letters
                out.append(' '); prev_space = True
        else:                                     # non-ASCII letters/symbols kept
            out.append(ch); prev_space = False
    return ''.join(out).strip()

def fingerprint(file_path: str, comment: str) -> str:
    s = f"{file_path}\x00{normalize_comment(comment)}".encode('utf-8')
    h = 0xcbf29ce484222325                         # FNV-1a 64-bit
    for b in s:
        h = ((h ^ b) * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
    return f"{h:016x}"
```

If this ever drifts from the Rust, every metric below is wrong — pin it with a
test that hashes a known pair and compares to a value produced by the engine.

---

## 3. Build the labeled dataset

```
for each trace file:
    group events by runId
    prKey, settings = from run_start
    for each finding_final / adjudicated_finding / guard_drop / suppressed:
        fp = event.fingerprint or fingerprint(filePath, comment)
        label = verdicts[(prKey, fp)].verdict   # else None
        emit row(prKey, fp, kind, stage_fields, confidence, severity, tier,
                 sources, reason?, surfaced=(kind in {finding_final}), label)
```

Outcome classes (the supervised signal):

| Row | Surfaced? | Label | Meaning |
|-----|-----------|-------|---------|
| `finding_final` | yes | accepted/edited | **true positive** |
| `finding_final` | yes | dismissed | **false positive** |
| `finding_final` | yes | none | unlabeled — reviewer didn't act (weak; track, exclude from precision) |
| `guard_drop` | no | — | a drop; *correctness* judged by cross-reference (§4) |
| `suppressed` | no | — | a suppression; correctness judged by cross-reference |

Treat `edited` as accepted-with-changes (count as TP) but track it separately —
a high edit rate means "right issue, wrong wording," which is a *prompt* signal,
not a *threshold* signal.

---

## 4. Metrics (with formulas)

Let surfaced-and-labeled findings be the evaluation set `E`.

- **Precision** `= |accepted ∪ edited| / |E|`, computed overall and stratified by
  confidence band (deciles), `severity`, `tier`, and each `specialist ∈ sources`
  (a finding credits every source).
- **Reliability / calibration curve:** bin `confidence` into deciles; for each
  bin plot empirical accept-rate. **Calibration error**
  `= mean_over_bins | bin.meanConfidence/100 − bin.acceptRate |`. A well-behaved
  adjudicator has accept-rate ≈ confidence.
- **Threshold sweep:** for `t` in `0..=100`,
  `precision(t) = acceptRate({f ∈ E : f.confidence ≥ t})`,
  `volume(t) = |{…}|`. Pick `t*` = smallest `t` with `precision(t) ≥ TARGET`
  (default `TARGET = 0.90`) and acceptable `volume(t)`.
- **Critical-line sweep:** same, restricted to `severity == critical`, → `c*`.
- **Guard efficacy** (uses recomputed fingerprints):
  - false-drop rate `= |{guard_drop : fp accepted/edited anywhere}| / |guard_drop|`,
    split by `reason`. A `below_threshold` drop whose fingerprint is accepted
    elsewhere ⇒ the threshold is too high. An `outside_hunk` drop accepted
    elsewhere ⇒ the anchor window is too tight (false positive of the check).
- **Suppression efficacy:** over-suppression rate
  `= |{suppressed : fp later accepted/edited}| / |suppressed|`. Any > 0 means a
  dismissed finding's fingerprint collided with a genuinely-wanted one.
- **Specialist value:** per specialist, `(precision, volume, unique)` where
  `unique` = findings whose `sources == [that specialist]`. Low precision *and*
  low unique ⇒ dead weight.
- **Cost:** `latencyMs` per `stage`; prompt size `len(messages_json)` as a token
  proxy; cost-per-surfaced-finding.
- **Parse health:** `llm_call(stage=adjudicate)` whose `response` doesn't yield a
  parseable object / yields zero findings while candidates existed ⇒ prompt or
  model JSON-compliance problem.

---

## 5. Decision rules

Each rule fires only above its minimum sample size `N`, and emits a
*recommendation* (never an auto-apply). `cur.*` = the value from `run_start.settings`.

| # | Condition | N | Recommendation |
|---|-----------|---|----------------|
| R1 | `|t* − cur.confidenceThreshold| ≥ 5` | 50 | set `ai_confidence_threshold = t*` |
| R2 | `|c* − cur.blockingConfidence| ≥ 5` | 30 | set `ai_blocking_confidence = c*` |
| R3 | specialist precision `< 0.35` | 20 | revise that specialist's prompt (use its dismissed rows as negative examples), or drop it from `THOROUGH_SPECIALISTS` |
| R4 | over-suppression rate `> 0` | 1 | inspect the colliding pairs; tighten `normalize_comment` or scope suppression more narrowly |
| R5 | `outside_hunk` false-drop rate `> 0.10` | 20 | widen the hunk context window / relax the anchor check |
| R6 | calibration error `> 0.15` | 50 | add the empirical recalibration map (§6) and/or a calibration instruction to the adjudicator prompt |
| R7 | edit rate among accepted `> 0.4` | 30 | the issues are right but the wording is off — distill accepted+edited pairs into the adjudicator's style guidance |
| R8 | any high-signal case (a dismissed FP, or a dropped/suppressed finding later accepted) | 1 | generate an eval fixture (§7) so the harness locks in the fix |

Tag every recommendation with scope: **global** vs **per-repo** (compute metrics
grouped by the repo segment of `prKey`; a pattern dismissed in one repo may be
valid in another, exactly as suppression is per-PR).

---

## 6. Confidence recalibration map (optional, deterministic)

If R6 fires, the model's self-reported confidence is biased. Build a monotonic
empirical map and apply it *before* thresholding/tiering:

```
bins = decile-bucket E by reported confidence
raw→real = { bin.midpoint : bin.acceptRate*100 }
calibrated(c) = isotonic/monotone-interpolate(raw→real, c)
```

Apply as a small lookup on `f.confidence` at the start of
`apply_finding_guards` (deterministic, testable), or feed the per-bin table back
into the adjudicator prompt as guidance. Either way, re-measure calibration error
on a held-out slice — never fit and evaluate on the same runs.

---

## 7. Prompt improvement & fixture generation (non-deterministic side)

The `llm_call` events carry the **full prompt and raw response**, and the
adjudicator prompt already contains the numbered file context. So real runs can
be turned into both training signal and regression tests:

- **Negative set (false positives):** collect `finding_final` rows with
  `dismissed`. Their `comment` + the `llm_call(adjudicate)` context are
  ready-made "do not flag this" examples for the adjudicator and the originating
  specialist (`sources`).
- **Positive set (true positives):** `accepted` rows — confirm the prompt keeps
  catching these (guard against regressions when you tighten for FPs).
- **Fixture synthesis (R8):** for a high-signal case, reconstruct the file from
  the numbered context inside the relevant `llm_call.messages`, write `old.txt`/
  `new.txt`, and set `expected.json` from the label (a dismissed FP → a
  `falsePositive: true` trap; a dropped-but-accepted finding → a true positive at
  that line). Drop it in `src-tauri/tests/eval/fixtures/`. The eval harness now
  regression-tests the exact case the field got wrong.

This is how the system grows its own eval set from production instead of guesses.

---

## 8. Reference processor (skeleton)

```python
def self_improve(diag_dir, db_path, settings):
    verdicts = load_verdicts(db_path)              # {(pr_key, fp): row}
    rows = []
    for f in glob(f"{diag_dir}/*.jsonl"):
        rows += label_run(parse_jsonl(f), verdicts)  # §2/§3, recompute fps
    m = metrics(rows)                               # §4
    recs = []
    if abs(m.t_star - settings.confidence_threshold) >= 5 and m.n >= 50:
        recs.append(("confidence_threshold", m.t_star))            # R1
    if abs(m.c_star - settings.blocking_confidence) >= 5 and m.n_crit >= 30:
        recs.append(("blocking_confidence", m.c_star))             # R2
    for s in m.specialists:
        if s.precision < 0.35 and s.n >= 20:
            recs.append(("revise_or_drop_specialist", s.label, s.dismissed_examples))  # R3
    recs += suppression_checks(m)                   # R4
    recs += anchor_checks(m)                        # R5
    recs += calibration_checks(m)                   # R6/R7
    fixtures = synth_fixtures(rows)                 # R8
    return Report(metrics=m, recommendations=recs, candidate_fixtures=fixtures)
```

Outputs: a human-readable `report.md`, a machine-readable `recommendations.json`,
and `candidate_fixtures/`.

---

## 9. Cadence, guardrails, drift

- **Cadence:** run after every ~50 new verdicts, or weekly.
- **Statistical discipline:** respect the `N` floors; report Wilson confidence
  intervals on every accept-rate; never act on a single noisy PR; hold out a
  slice when fitting the recalibration map.
- **Gate:** apply a recommendation only if the eval harness
  (`PEX_EVAL=1 cargo run --example eval_review`) shows precision ↑ with recall
  not materially ↓ on the fixture set.
- **Model drift:** a model upgrade invalidates the calibration map and may shift
  every distribution — re-baseline (recompute metrics on post-upgrade runs only)
  before trusting old thresholds. Stamp `run_start` with the model so you can
  segment by it.
- **Privacy:** traces and the derived datasets contain source content — keep
  them local, and don't commit raw traces. Only curated, reviewed fixtures go in
  the repo.
