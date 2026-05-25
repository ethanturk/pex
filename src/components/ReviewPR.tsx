import { useState, useEffect, useRef } from "preact/hooks";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  startReview,
  startReviewPost,
  cancelReview,
  getSavedReview,
  clearSavedReview,
  type ReviewOutput,
} from "@/lib/api";

interface Props {
  prId: number;
  prTitle: string;
}

type ReviewState = "idle" | "running" | "done" | "posting" | "posted" | "error";

interface Progress {
  phase: string;
  detail: string;
  fileNum?: number;
  totalFiles?: number;
  hunk?: number;
  totalHunks?: number;
  batch?: number;
  totalBatches?: number;
  fileCount?: number;
}

export function ReviewPR({ prId, prTitle }: Props) {
  const [state, setState] = useState<ReviewState>("idle");
  const [progress, setProgress] = useState<Progress | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showOutput, setShowOutput] = useState(false);
  const [reviewOutput, setReviewOutput] = useState<ReviewOutput | null>(null);
  const [hasSavedState, setHasSavedState] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);

  // Check for resumable state on mount
  useEffect(() => {
    getSavedReview().then((s) => setHasSavedState(!!s));
  }, []);

  // Auto-scroll panel
  useEffect(() => {
    if (panelRef.current) {
      panelRef.current.scrollTop = panelRef.current.scrollHeight;
    }
  }, [progress]);

  const handleStartReview = async () => {
    setState("running");
    setError(null);
    setReviewOutput(null);
    setShowOutput(true);

    const unlisteners: UnlistenFn[] = [];

    // Listen for progress events
    const unlistenProgress = await listen<Progress>("review-progress", (event) => {
      setProgress(event.payload);
    });
    unlisteners.push(unlistenProgress);

    // Listen for completion
    const unlistenDone = await listen<{ success: boolean; summary: string; findings: any[] }>(
      "review-done",
      (event) => {
        if (event.payload.success) {
          setReviewOutput({
            summary: event.payload.summary,
            findings: event.payload.findings,
          });
          setState("done");
        } else {
          setState("error");
          setError("Review failed");
        }
        unlisteners.forEach((u) => u());
      }
    );
    unlisteners.push(unlistenDone);

    try {
      await startReview(prId, prTitle);
    } catch (e: any) {
      setState("error");
      setError(String(e));
      unlisteners.forEach((u) => u());
    }
  };

  const handlePostReview = async () => {
    setState("posting");
    setError(null);

    const unlisteners: UnlistenFn[] = [];

    const unlistenProgress = await listen<Progress>("review-progress", (event) => {
      setProgress(event.payload);
    });
    unlisteners.push(unlistenProgress);

    const unlistenDone = await listen<{ success: boolean; message: string }>(
      "review-post-done",
      (event) => {
        if (event.payload.success) {
          setState("posted");
        } else {
          setState("error");
          setError(event.payload.message);
        }
        unlisteners.forEach((u) => u());
      }
    );
    unlisteners.push(unlistenDone);

    try {
      await startReviewPost(prId, prTitle);
    } catch (e: any) {
      setState("error");
      setError(String(e));
      unlisteners.forEach((u) => u());
    }
  };

  const handleCancel = () => {
    cancelReview();
  };

  const handleResume = async () => {
    setHasSavedState(false);
    await handleStartReview();
  };

  const handleDiscard = async () => {
    await clearSavedReview();
    setHasSavedState(false);
  };

  const handleClose = () => {
    setShowOutput(false);
    setState("idle");
    setProgress(null);
    setReviewOutput(null);
    setError(null);
  };

  const progressText = () => {
    if (!progress) return "Starting review...";
    switch (progress.phase) {
      case "resume":
        return "Resuming from saved progress...";
      case "hunk-review":
        return `Reviewing ${progress.detail} — hunk ${progress.hunk}/${progress.totalHunks}`;
      case "file-aggregate":
        return progress.detail;
      case "batch-aggregate":
        return progress.detail;
      case "synthesis":
        return "Producing final review summary...";
      case "posting":
        return "Posting findings to ADO...";
      case "done":
        return "Review complete";
      default:
        return progress.detail;
    }
  };

  return (
    <div>
      {/* Trigger button */}
      <button
        onClick={handleStartReview}
        disabled={state === "running" || state === "posting"}
        class="px-3 py-1.5 bg-accent hover:bg-accent-hover text-white rounded-lg text-xs font-medium disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-1.5"
      >
        {state === "running" ? (
          <>
            <span class="animate-spin w-3 h-3 border-2 border-white/30 border-t-white rounded-full" />
            Reviewing...
          </>
        ) : state === "posting" ? (
          <>
            <span class="animate-spin w-3 h-3 border-2 border-white/30 border-t-white rounded-full" />
            Posting...
          </>
        ) : (
          "🔍 Review PR"
        )}
      </button>

      {/* Resumable indicator */}
      {hasSavedState && state === "idle" && (
        <span class="text-xs text-amber-500 ml-2">
          Review in progress —
          <button onClick={handleResume} class="underline ml-1">resume</button>
          {" · "}
          <button onClick={handleDiscard} class="underline">discard</button>
        </span>
      )}

      {/* Output modal */}
      {showOutput && (
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={handleClose}>
          <div
            class="bg-white dark:bg-gray-900 rounded-xl shadow-xl border border-gray-200 dark:border-gray-700 w-full max-w-2xl mx-4 max-h-[80vh] flex flex-col"
            onClick={(e) => e.stopPropagation()}
          >
            {/* Header */}
            <div class="flex items-center justify-between px-5 py-3 border-b border-gray-200 dark:border-gray-700 shrink-0">
              <h2 class="text-sm font-semibold">
                {state === "posted" ? "Review Posted ✓" : state === "done" ? "Review Complete" : "PR Review"}
              </h2>
              <div class="flex items-center gap-2">
                {(state === "running" || state === "posting") && (
                  <button
                    onClick={handleCancel}
                    class="px-2 py-1 text-xs text-red-600 dark:text-red-400 border border-red-300 dark:border-red-700 rounded hover:bg-red-50 dark:hover:bg-red-900/30"
                  >
                    Cancel
                  </button>
                )}
                {state === "done" && (
                  <button
                    onClick={handlePostReview}
                    class="px-3 py-1.5 bg-green-600 hover:bg-green-700 text-white rounded text-xs font-medium"
                  >
                    Post findings to ADO
                  </button>
                )}
                <button
                  class="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-lg leading-none"
                  onClick={handleClose}
                >
                  ×
                </button>
              </div>
            </div>

            {/* Output content */}
            <div ref={panelRef} class="flex-1 overflow-y-auto p-4 text-sm">
              {error ? (
                <div class="text-red-600 dark:text-red-400 whitespace-pre-wrap">{error}</div>
              ) : (
                <>
                  {/* Progress bar */}
                  {(state === "running" || state === "posting") && (
                    <div class="mb-4">
                      <div class="flex items-center gap-2 text-gray-400">
                        <span class="animate-spin w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full" />
                        <span>{progressText()}</span>
                      </div>
                      {progress?.totalFiles && (
                        <div class="mt-2 bg-gray-200 dark:bg-gray-700 rounded-full h-1.5 overflow-hidden">
                          <div
                            class="bg-accent h-full rounded-full transition-all duration-300"
                            style={{
                              width: progress.phase === "hunk-review"
                                ? `${Math.round(
                                    ((progress.fileNum! - 1 + (progress.hunk || 0) / (progress.totalHunks || 1)) /
                                      progress.totalFiles) *
                                      100
                                  )}%`
                                : progress.phase === "batch-aggregate"
                                  ? `${Math.round(((progress.batch || 0) / (progress.totalBatches || 1)) * 100)}%`
                                  : "0%",
                            }}
                          />
                        </div>
                      )}
                    </div>
                  )}

                  {/* Review summary */}
                  {reviewOutput?.summary && (
                    <div class="whitespace-pre-wrap text-gray-700 dark:text-gray-300 leading-relaxed">
                      {reviewOutput.summary}
                    </div>
                  )}

                  {/* Status at bottom */}
                  {(state === "running" || state === "posting") && !progress && (
                    <div class="text-gray-400">
                      <span class="animate-spin inline-block w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full mr-2" />
                      Starting...
                    </div>
                  )}
                </>
              )}
            </div>

            {/* Footer */}
            {(state === "done" || state === "posted" || state === "error") && (
              <div class="px-5 py-3 border-t border-gray-200 dark:border-gray-700 shrink-0">
                <button
                  onClick={handleClose}
                  class="px-3 py-1.5 text-xs border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-800"
                >
                  Close
                </button>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
