# Phase 1 Design — Precision Foundation

> Make findings REAL: cut false positives, score confidence, prove it with an
> eval harness. Prerequisite for triage (Phase 2) and the trust loop (Phase 3).
> See [`../ROADMAP.md`](../ROADMAP.md) for the long-horizon arc.

## Goal & exit criteria

Today every candidate finding flows straight to the reviewer, scored only by a
coarse 3-level `Severity`, generated from a hunk viewed in isolation. Phase 1
closes the three gaps that cap trust:

1. **Hunk isolation** → false positives (the model can't see the rest of the file).
2. **No verification gate** → confident-but-wrong findings reach the reviewer.
3. **No confidence** → no way to separate "sure" from "guess," so no noise floor.

**Exit criteria**

- Every finding carries a `confidence` (0–100) alongside `severity`.
- A configurable threshold (default **80**) filters findings before they surface.
- Each hunk pass and the adjudicator see surrounding file context.
- The sixth toolkit specialist (`code-simplifier`) is wired in.
- An eval harness reports precision / recall against a labeled fixture set, and
  the false-positive rate is measurably lower than baseline.

---

## The confidence model (adopted from the toolkit)

The `pr-review-toolkit` `code-reviewer` scores issues 0–100 and reports only
≥80. Pex adopts that scale but keeps **severity and confidence as separate
axes**, because Phase 2 triage needs both:

- **`severity`** — *how bad if true* (impact). Existing `Critical/Moderate/Minor`.
- **`confidence`** — *how sure we are* (likelihood), 0–100.

Confidence rubric handed to the adjudicator (mirrors the toolkit):

| Band | Meaning |
|------|---------|
| 0–25 | Likely false positive or pre-existing issue |
| 26–50 | Minor suggestion, not mandated by guidelines |
| 51–75 | Valid but low-impact |
| 76–90 | Important, warrants attention |
| 91–100 | Critical bug or explicit guideline violation |

Default reporting threshold: **80**. This single dial is Pex's noise floor —
and the dial Phase 3 will tune *with evidence*.

---

## Data model changes

### Rust (`src-tauri/src/review/engine.rs`)

Add `confidence` (and an internal-only `evidence`) to both finding structs:

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAggregateFinding {
    pub severity: Severity,
    #[serde(default = "default_confidence")]
    pub confidence: u8,                 // 0..=100
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub comment: String,
    /// Cited new-side line(s) the adjudicator used to justify the finding.
    /// Used by the deterministic anchor check + logging; stripped before posting.
    #[serde(default)]
    pub evidence: Option<String>,
}

fn default_confidence() -> u8 { 80 } // legacy findings predate scoring → treat as at-threshold
```

Mirror the same `confidence` field on `Finding` (the flattened, path-injected
type). `default_confidence` = 80 keeps already-completed files (persisted in
`ReviewState.completed_files`) surfacing exactly as before after an upgrade.

### Resumable state back-compat (`src-tauri/src/review/state.rs`)

`ReviewState` is serialized as JSON into `settings['review_state']`.
`load_state` already discards on parse error, but adding defaulted fields means
old in-flight states will now *load* with defaults rather than error. To avoid
a half-old/half-new state resuming incoherently, add an explicit version and
discard mismatches:

```rust
#[serde(default)] pub schema_version: u32,   // bump to 2 this phase
```

In `load_state`, if `schema_version` < current, `clear_state` and return `None`
(start fresh). Completed findings are cheap to regenerate; a corrupt resume is
not worth the risk.

### Frontend (`src/lib/api.ts`)

```ts
export interface ReviewFinding {
  filePath: string;
  severity: Severity;
  confidence: number;          // 0..=100
  lineStart: number | null;
  lineEnd: number | null;
  comment: string;
}
```

`PRReviewSidebar` already groups by severity; in Phase 1 it just **displays**
confidence (e.g. a muted `82%` next to the severity badge). Re-ranking and the
nit rollup are Phase 2 — keep the sidebar's grouping logic untouched here.

---

## 1.1 — Kill hunk isolation (full-file context)

`review_single_hunk` builds its user message from `hunk.header` + hunk lines
only. Give each pass a window of the surrounding file so cross-hunk facts are
visible.

- Thread the file's `new_content` (and `old_content` when useful) from
  `FileInput` — already fetched in `commands/review.rs::fetch_file_inputs` —
  down into `review_single_hunk`. Today only `file_path` is passed.
- Build a **context window**: the hunk plus ±`context_window` lines of the new
  file around it, clearly delimited, with the hunk-under-review marked. For
  small files, pass the whole file. Cap by a configurable char budget
  (mirror `standards::truncate`).
- Prompt framing: *"Here is the surrounding file for context. Review ONLY the
  marked hunk; use the rest solely to avoid false positives (e.g. a symbol
  defined elsewhere, an error handled by the caller)."*

This is plumbing, not new pipeline surface, and it directly attacks the #1
false-positive class.

## 1.2 — Adjudicator / verification gate

Rather than add a brand-new stage, **evolve the existing file-aggregate step
into an adjudicator** (`FILE_AGGREGATE_SYSTEM` in `review/prompts.rs`). It
already converts per-hunk prose into structured per-file JSON; we make it
verify before it emits.

Inputs gain the file content (the same windowed/capped context as 1.1) via
`file_aggregate_user_message`. New contract:

- For each candidate, produce **claim → evidence (cited new-side lines) →
  verdict**, assigning `severity` + `confidence` per the rubric.
- **Drop** any finding whose cited lines don't substantiate it, or that the
  full file shows is already handled. Set low confidence rather than inventing.
- Merge duplicates across specialists (already requested; now enforced by
  giving the adjudicator the evidence to dedupe on).

New JSON schema (extends the current one):

```json
{
  "summary": "…", "verdict": "approve|review-required|needs-work",
  "findings": [{
    "severity": "critical|moderate|minor",
    "confidence": 0,                 // 0..=100 per rubric
    "lineStart": 0, "lineEnd": 0,    // new-side, or null if file-level
    "evidence": "new-side line(s) justifying this",
    "comment": "standalone inline comment"
  }]
}
```

**Deterministic guards in Rust** (defense in depth, no extra LLM cost), applied
in the engine after parsing the aggregate:

1. **Threshold filter** — drop `confidence < threshold` (default 80).
2. **Anchor check** — if `lineStart` is set, verify it falls within a reviewed
   hunk's new-side range (`DiffHunk.new_start .. new_start+new_count`). If not,
   the model likely hallucinated a line: downgrade confidence below threshold
   (effectively drop) and log it. File-level findings (`null` lines) are exempt.
3. Strip `evidence` before posting (`post_findings`) — it's internal.

Net effect: a finding must be *grounded in cited lines that exist in the
changed region and clear the confidence bar* before a reviewer ever sees it.

## 1.3 — Confidence as a first-class field

Covered by the data-model changes plus:

- New setting `ai_confidence_threshold` (default 80, clamp 0–100) with a
  `read_confidence_threshold` helper next to `read_retry_count` in
  `ai/mod.rs`; add to `AiSettingsNoKey`, the get/set AI-settings commands, and
  the AI settings form (`src/components/AiSettings.tsx`).
- Resolved once per run in `run_review` (like `retry_count` / `hunk_concurrency`).

## 1.4 — Add the sixth specialist: `code-simplifier`

Complete the toolkit set (`ai/prompts.rs`):

- Add `ReviewCodeSimplifierSystem` to `PromptKey`, to `ALL`, and to
  `THOROUGH_SPECIALISTS`; add `as_str`/`from_str`/`specialist_label`/
  `default_text` arms.
- Quality-only lens (reuse, redundancy, dead code, clarity) — mirrors the
  `/simplify` skill. It must obey the same `No issues found.` sentinel and the
  confidence rubric so it can't become nit-noise; the threshold filter is its
  backstop.
- Per-prompt model override already works for free via `resolve_model`.

## 1.5 — Evaluation harness (built alongside 1.1–1.4)

You cannot improve precision you don't measure. Ship a thin harness now.

- **Fixtures**: `src-tauri/tests/eval/fixtures/<case>/` each holding
  `old.txt`, `new.txt`, and `expected.json` (labeled true positives + known
  false positives with line ranges and a match tolerance). Seed ~20–30 cases
  from real PRs, including known past FPs.
- **Runner**: an example binary (pattern already in
  `src-tauri/examples/probe_diff.rs`) or a `#[test]` gated behind `PEX_EVAL=1`,
  since it calls a live provider. Runs the single-file review path
  (hunk passes → adjudicator) per fixture, matches emitted findings to expected
  by file+line overlap, and prints a scorecard: **precision, recall,
  FP list, per-specialist precision**.
- **Determinism**: temperature 0 where the provider supports it; record raw
  model output per case for diffing prompt changes.
- **Use**: run before/after each prompt or model change in this phase; the
  before/after FP count is the proof Phase 1 worked.

---

## Pipeline after Phase 1

```
hunk passes (Fast: 1 / Thorough: 6)   ← now with file-context window  (1.1, 1.4)
        │  prose candidates per hunk
        ▼
file adjudicator  ← full-file context; emits severity+confidence+evidence;
        │           drops ungrounded findings                          (1.2)
        ▼
deterministic guards  ← threshold filter + anchor check + strip evidence (1.2, 1.3)
        ▼
batch-aggregate → final synthesis        (unchanged this phase)
```

Phases 2–4 build on the `confidence` field and the adjudicator established here.

---

## Testing & rollout

- **Rust units**: `default_confidence` back-compat; threshold filter; anchor
  check (in-range kept, out-of-range dropped, `null` exempt); aggregate JSON
  parse with/without `confidence`/`evidence`; `schema_version` discard path.
- **Serde back-compat**: deserialize a v1 `ReviewState` and a pre-confidence
  `FileAggregateResult` — confirm no panic and sane defaults.
- **Frontend**: `ReviewFinding` renders confidence; posting omits `evidence`.
- **Eval harness**: baseline scorecard captured before changes; target a clear
  FP-rate drop with no meaningful recall loss on the fixture set.
- **Rollout**: threshold defaults to 80 but is user-tunable; `code-simplifier`
  ships enabled in Thorough only. Fast mode keeps a single pass (now
  context-aware) for users on slow/local providers.

## Risks & mitigations

- **Token budget / latency** from file context → cap with a char budget +
  windowing; whole-file only under the cap. Reuse `standards::truncate`.
- **Model omits `confidence`** → `serde(default)` = 80 (at threshold), and the
  prompt makes it mandatory; eval harness catches regressions.
- **Over-filtering hides real issues** → anchor check only *downgrades* on
  clear hallucination; threshold is configurable; recall tracked by the harness.
- **Adjudicator latency** (now reads file content) → it already runs once per
  file; context is capped, and it replaces, not adds to, the aggregate call.

## Suggested build order

1. Data model + serde back-compat + `schema_version` (unblocks everything).
2. Confidence threshold setting + `read_confidence_threshold`.
3. Adjudicator prompt + deterministic guards (1.2, 1.3).
4. Full-file context window (1.1).
5. `code-simplifier` specialist (1.4).
6. Eval harness + baseline (1.5) — start in parallel with 1; it's how we grade
   the rest.
