# Diagnostic Traces — for Evaluation & Tuning

> Opt-in telemetry that records what a review run actually did, so its behaviors
> — both the non-deterministic LLM calls and the deterministic decisions — can
> be measured, tuned, and turned into evaluation data from real runs. Enable via
> **AI settings → "Write diagnostic traces."** Off by default.

## Where & format

One **JSONL** file per review run (one JSON object per line) in
`<app-data>/diagnostics/<timestamp>-pr<id>.jsonl`. The exact directory is shown
under the settings toggle and returned by the `get_diagnostics_dir` command.
Lines are flushed as they're written, so a crash mid-run still leaves a usable
partial trace.

Every line has `ts` (RFC3339), `runId`, and `kind`, plus kind-specific fields.

## Event kinds

| `kind` | When | Key fields |
|--------|------|------------|
| `run_start` | once, at the top | `prKey`, `prTitle`, `mode`, `fileCount`, `settings` (`confidenceThreshold`, `blockingConfidence`, `retryCount`, `hunkConcurrency`) |
| `hunk_candidate` | per surfaced hunk finding | `filePath`, `hunk`, `text` (the raw per-hunk specialist prose, `[label]`-tagged in Thorough) |
| `llm_call` | per adjudicate / batch / synthesis call | `stage`, `latencyMs`, `messages` (full prompt sent), `response` (raw model output), `filePath`/`batch` where relevant |
| `token_usage` | per LLM call that reported usage | `stage` (`hunk`, `specialist:<label>`, `adjudicate`, `batch`, `synthesis`, `anchor`), `inputTokens`, `outputTokens` |
| `adjudicated_finding` | per parsed finding the guards **kept** | `filePath`, `severity`, `confidence`, `lineStart`/`lineEnd`, `sources`, `comment` |
| `guard_drop` | per parsed finding the guards **dropped** | same as above **plus** `reason` = `below_threshold` \| `outside_hunk` |
| `suppressed` | per finding dropped by suppression memory | `filePath`, `fingerprint`, `lineStart`, `tier`, `comment` |
| `finding_final` | per surviving finding | `severity`, `confidence`, `tier`, `lineStart`/`lineEnd`, `sources`, `comment`, **`fingerprint`** |
| `run_done` | once, at the end | `totalFiles`, `findings`, `suppressed`, `blocking`/`shouldFix`/`nit`/`fyi`, `inputTokens`, `outputTokens`, `llmCalls`, `callsWithoutUsage` |

## Deterministic vs non-deterministic coverage

- **Non-deterministic** (the model): `llm_call` captures the exact prompt and raw
  response for the adjudicate, batch, and synthesis stages; `hunk_candidate`
  captures what the per-hunk specialists produced. This is the data for studying
  prompt → output behavior and for distilling new eval fixtures.
- **Deterministic** (the engine): `adjudicated_finding` / `guard_drop` (with
  `reason`), `suppressed`, and `finding_final` record every threshold, anchor,
  tier, and suppression decision — the signal for tuning the confidence
  threshold, the critical line, and the anchor check.

## Evaluating token cost

The `token_usage` events plus the `run_done` totals make a trace a record of
what a review *cost*. To turn a directory of traces into a per-stage / per-run
token report (and the derived output-tokens-per-file figure), run the bundled
evaluator:

```bash
cd src-tauri
cargo run --example eval_traces                 # scans the default diagnostics dir
cargo run --example eval_traces -- run-a.jsonl run-b.jsonl
```

It reads existing traces only (never calls an LLM), so it's the way to confirm
a prompt / cap / specialist-roster change actually moved token cost rather than
estimating from tokens/sec.

## The join that makes it training data

`finding_final` events carry the **same `fingerprint`** (`file_path` +
normalized comment) that the feedback loop stores on `finding_verdicts`. So a
trace (the *features*: confidence, tier, sources, which guard fired) can be
joined by `(prKey, fingerprint)` to the reviewer's recorded **verdict** (the
*label*: accepted / dismissed / edited). That join is a labeled dataset of
"finding → was it real?", which is exactly what's needed to:

- measure precision/recall per confidence band, severity, tier, and specialist;
- choose the confidence threshold and critical line from data, not guesses;
- spot guard drops that were actually correct (or wrong) by checking whether a
  dropped fingerprint was later re-raised and accepted.

### Sketch (jq + the SQLite verdicts)

```bash
# Final findings with their features, one per line:
jq -c 'select(.kind=="finding_final")' diagnostics/*.jsonl > findings.jsonl
# Join to verdicts (sources of truth for labels):
sqlite3 -json "$PEX_DB" \
  "SELECT pr_key, fingerprint, verdict, severity, tier, confidence FROM finding_verdicts" > verdicts.json
# → join on fingerprint to get (features, label) rows for analysis.
```

## Privacy & size

Traces include source content and full prompts, so they're opt-in and written
only locally. They can be large for big PRs in Thorough mode. Delete the
`diagnostics/` directory anytime; nothing depends on past traces.

## Known gap (future)

Per-*specialist* prompt-level capture (the exact messages sent to each
specialist inside `review_single_hunk`) is not yet recorded — only the combined,
`[label]`-tagged `hunk_candidate` output is. Threading the diagnostics sink into
`review_single_hunk` would add it; deferred to keep the instrumentation off the
hottest, most-parallel path for now.
