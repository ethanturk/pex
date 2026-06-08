// Phase 4 auto-review orchestrator.
//
// When auto-review is enabled, PRs with a new iteration are reviewed in the
// background, one at a time (the Rust engine serializes runs, and we serialize
// the queue here so we never stampede the provider). When a review finishes,
// the high-confidence blocking findings are optionally auto-posted; everything
// else waits in the sidebar for the human gate.

import {
  startReview,
  autoReviewCandidates,
  autoPostReviewFindings,
  type ReviewMode,
} from "@/lib/api";
import { activeReviewPrId, reviewRuns, updateReviewRun } from "@/lib/signals";

interface QueueItem {
  projectId: string;
  repoId: string;
  prId: number;
  prTitle: string;
}

const queue: QueueItem[] = [];
const queued = new Set<number>();
let draining = false;

/// Consider the listed PRs for auto-review and enqueue the ones the backend says
/// need it. Safe to call on every PR-list load/refresh — already-queued or
/// in-flight PRs are skipped, and it no-ops when auto-review is disabled.
export async function considerAutoReview(
  projectId: string,
  repoId: string,
  prs: { prId: number; prTitle: string; iterationCount: number }[],
): Promise<void> {
  if (prs.length === 0) return;
  let candidates: number[];
  try {
    candidates = await autoReviewCandidates(
      projectId,
      repoId,
      prs.map((p) => ({ prId: p.prId, iterationCount: p.iterationCount })),
    );
  } catch {
    return; // disabled, not authenticated, or transient — try again next refresh
  }

  for (const prId of candidates) {
    if (queued.has(prId)) continue;
    // Don't re-review something that already has a result/run this session.
    if (reviewRuns.value.get(prId)) continue;
    const pr = prs.find((p) => p.prId === prId);
    if (!pr) continue;
    queued.add(prId);
    queue.push({ projectId, repoId, prId, prTitle: pr.prTitle });
  }

  void drain();
}

async function drain(): Promise<void> {
  if (draining) return;
  draining = true;
  try {
    while (queue.length > 0) {
      // Back off while any review (user- or auto-initiated) is in flight; the
      // next considerAutoReview call will resume the queue.
      if (activeReviewPrId.value !== null) break;
      const item = queue.shift()!;
      queued.delete(item.prId);
      await runOne(item);
    }
  } finally {
    draining = false;
  }
}

async function runOne({ projectId, repoId, prId, prTitle }: QueueItem): Promise<void> {
  const mode: ReviewMode = "fast";
  activeReviewPrId.value = prId;
  const next = new Map(reviewRuns.value);
  next.set(prId, {
    projectId,
    repoId,
    prTitle,
    status: "running",
    progress: { phase: "auto", detail: "Auto-reviewing…" },
    output: null,
    error: null,
    warnings: [],
    mode,
  });
  reviewRuns.value = next;

  try {
    const output = await startReview(projectId, repoId, prId, prTitle, mode);
    updateReviewRun(prId, { status: "done", output, progress: null });
    // Earned autonomy: post only the highest-confidence blockers; the rest stay
    // for the human gate. No-op unless the user enabled auto-post.
    try {
      await autoPostReviewFindings(projectId, repoId, prId, output.findings);
    } catch {
      // best-effort; the findings remain in the sidebar to post manually
    }
  } catch (e: unknown) {
    updateReviewRun(prId, { status: "error", error: String(e) });
  } finally {
    if (activeReviewPrId.value === prId) activeReviewPrId.value = null;
  }
}
