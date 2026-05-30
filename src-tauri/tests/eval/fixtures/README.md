# Review eval fixtures

Each subdirectory is one labeled case for the `eval_review` harness
(`src-tauri/examples/eval_review.rs`). A case has three files:

| File | Meaning |
|------|---------|
| `old.txt` | the file's base content (empty = a newly added file) |
| `new.txt` | the file's content under review |
| `expected.json` | the labels (see below) |

`expected.json`:

```json
{
  "path": "config.py",
  "expected": [
    { "lineStart": 5, "lineEnd": 5, "falsePositive": false, "note": "divide by zero on empty input" },
    { "lineStart": 6, "lineEnd": 6, "falsePositive": true,  "note": "error is handled by the caller; must NOT be flagged" }
  ]
}
```

- `falsePositive: false` (or omitted) → a **true positive** the engine *should* surface.
  Recall counts how many of these it caught.
- `falsePositive: true` → a **known trap** the engine must *not* flag. Any finding
  overlapping it is reported as a false-positive regression.
- Line numbers are 1-based, new-side, inclusive. Use `null` for genuinely
  file-level findings.

Matching is by line-range overlap, so labels can be approximate (±a line or two
is fine).

## Running

```bash
cd src-tauri
PEX_EVAL=1 PEX_AI_KEY=... PEX_AI_MODEL=gpt-4.1 cargo run --example eval_review
```

Capture the scorecard before a prompt/model change and again after; the
before/after precision and false-positive counts are the proof a change helped.
Grow this set from real PRs — especially past false positives — so the harness
keeps pace with what the engine gets wrong in practice.
