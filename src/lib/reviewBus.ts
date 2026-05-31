import { listen } from "@tauri-apps/api/event";
import {
  activeReviewPrId,
  reviewRuns,
  updateReviewRun,
  type ReviewProgress,
} from "@/lib/signals";
import {
  startReview,
  startReviewPost,
  type ReviewMode,
  type ReviewOutput,
} from "@/lib/api";

let initialized = false;

// Wire global listeners exactly once. Events are routed to whichever PR is
// currently `activeReviewPrId` — the Rust engine serializes runs, so at most
// one PR is in flight at a time and there is no ambiguity.
export async function initReviewBus() {
  if (initialized) return;
  initialized = true;

  await listen<ReviewProgress>("review-progress", (e) => {
    const id = activeReviewPrId.value;
    if (id == null) return;
    updateReviewRun(id, { progress: e.payload });
  });

  // `review-done` is informational — the awaited promise from `startReview`
  // is the authoritative source of the final ReviewOutput, so we only use
  // this event to surface an explicit failure flag if Rust ever sends one.
  await listen<{ success: boolean; summary: string; findings: ReviewOutput["findings"] }>(
    "review-done",
    (e) => {
      const id = activeReviewPrId.value;
      if (id == null || e.payload.success) return;
      updateReviewRun(id, { status: "error", error: "Review failed" });
    },
  );

  await listen<{ success: boolean; message: string }>("review-post-done", (e) => {
    const id = activeReviewPrId.value;
    if (id == null) return;
    updateReviewRun(id, {
      status: e.payload.success ? "posted" : "error",
      error: e.payload.success ? null : e.payload.message,
    });
  });
}

export function startBackgroundReview(
  projectId: string,
  repoId: string,
  prId: number,
  prTitle: string,
  resuming = false,
  mode: ReviewMode = "fast",
  enabledSpecialists?: string[],
) {
  activeReviewPrId.value = prId;
  const next = new Map(reviewRuns.value);
  next.set(prId, {
    projectId,
    repoId,
    prTitle,
    status: "running",
    // Seed the progress so the sidebar shows "Resuming…" immediately
    // instead of "Starting review…" until the engine emits its first event.
    progress: resuming
      ? { phase: "resume", detail: "Resuming from saved progress..." }
      : null,
    output: null,
    error: null,
    mode,
  });
  reviewRuns.value = next;

  startReview(projectId, repoId, prId, prTitle, mode, enabledSpecialists)
    .then((output) => {
      updateReviewRun(prId, { status: "done", output, progress: null });
    })
    .catch((e: unknown) => {
      updateReviewRun(prId, { status: "error", error: String(e) });
    })
    .finally(() => {
      if (activeReviewPrId.value === prId) activeReviewPrId.value = null;
    });
}

export function postBackgroundReview(
  projectId: string,
  repoId: string,
  prId: number,
  prTitle: string,
  mode: ReviewMode = "fast",
) {
  activeReviewPrId.value = prId;
  updateReviewRun(prId, { status: "posting", error: null });

  startReviewPost(projectId, repoId, prId, prTitle, mode)
    .catch((e: unknown) => {
      updateReviewRun(prId, { status: "error", error: String(e) });
    })
    .finally(() => {
      if (activeReviewPrId.value === prId) activeReviewPrId.value = null;
    });
}
