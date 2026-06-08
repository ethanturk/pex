import { listen } from "@tauri-apps/api/event";
import {
  activeReviewPrId,
  reviewRuns,
  updateReviewRun,
  reconcilePersistedReviews,
  type PRReviewRun,
  type ReviewProgress,
  type ReviewWarning,
} from "@/lib/signals";
import {
  startReview,
  startReviewPost,
  listCompletedReviews,
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

  // A sync pull may have brought in review rows another device changed (e.g. a
  // review marked completed on another device). Re-read the persisted reviews
  // and reconcile them into `reviewRuns` so the badge/Review tab update without
  // a restart.
  await listen("db-synced", () => {
    listCompletedReviews().then(reconcilePersistedReviews).catch(() => {});
  });

  await listen<ReviewProgress>("review-progress", (e) => {
    const id = activeReviewPrId.value;
    if (id == null) return;
    const p = e.payload;
    const run = reviewRuns.value.get(id);
    // Always refresh the headline progress; some phases also update the
    // persistent per-file checklist tracked alongside it.
    const patch: Partial<PRReviewRun> = { progress: p };

    if (p.phase === "plan" && Array.isArray(p.files)) {
      patch.fileList = p.files;
      patch.fileDurations = {};
      patch.fileAnchors = {};
      patch.ruleTitles = p.ruleTitles ?? {};
      patch.preCompletedCount = p.completedCount ?? 0;
      patch.activeFileIndices = [];
      patch.activeFileStartMs = {};
    } else if (p.phase === "hunk-review" && typeof p.fileNum === "number") {
      // First sighting of a file → add it to the active set and start its timer.
      // Several files review concurrently, so the set can hold more than one.
      const idx = p.fileNum - 1;
      if (!(run?.activeFileIndices ?? []).includes(idx)) {
        patch.activeFileIndices = [...(run?.activeFileIndices ?? []), idx];
        patch.activeFileStartMs = {
          ...(run?.activeFileStartMs ?? {}),
          [idx]: Date.now(),
        };
      }
    } else if (p.phase === "file-done" && typeof p.fileIndex === "number") {
      patch.fileDurations = {
        ...(run?.fileDurations ?? {}),
        [p.fileIndex]: p.durationMs ?? 0,
      };
      patch.fileAnchors = {
        ...(run?.fileAnchors ?? {}),
        [p.fileIndex]: {
          kept: p.keptFindings ?? 0,
          anchored: p.anchoredFindings ?? 0,
          dropped: p.droppedFindings ?? 0,
          deterministic: p.deterministicFindings ?? 0,
        },
      };
      // Drop just this file from the active set; siblings stay in flight.
      patch.activeFileIndices = (run?.activeFileIndices ?? []).filter(
        (i) => i !== p.fileIndex,
      );
      const { [p.fileIndex]: _done, ...restStarts } = run?.activeFileStartMs ?? {};
      patch.activeFileStartMs = restStarts;
    }

    updateReviewRun(id, patch);
  });

  await listen<ReviewWarning>("review-warning", (e) => {
    const id = activeReviewPrId.value;
    if (id == null) return;
    const run = reviewRuns.value.get(id);
    updateReviewRun(id, {
      warnings: [...(run?.warnings ?? []), e.payload],
    });
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
    warnings: [],
    mode,
  });
  reviewRuns.value = next;

  // `resuming` is the caller's intent, not just a UI hint: pass it through so
  // the engine continues from saved progress only when the user chose to resume.
  startReview(projectId, repoId, prId, prTitle, mode, enabledSpecialists, resuming)
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
