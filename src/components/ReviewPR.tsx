import { useState, useEffect, useRef } from "preact/hooks";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { reviewPrDryRun, reviewPrPost, cancelReview } from "@/lib/api";

interface Props {
  orgUrl: string;
  project: string;
  repo: string;
  prId: number;
}

type ReviewState = "idle" | "running" | "done" | "posting" | "posted" | "error";

export function ReviewPR({ orgUrl, project, repo, prId }: Props) {
  const [state, setState] = useState<ReviewState>("idle");
  const [output, setOutput] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [showOutput, setShowOutput] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);

  // Auto-scroll panel
  useEffect(() => {
    if (panelRef.current) {
      panelRef.current.scrollTop = panelRef.current.scrollHeight;
    }
  }, [output]);

  const startReview = async () => {
    setState("running");
    setOutput([]);
    setError(null);
    setShowOutput(true);

    const unlisteners: UnlistenFn[] = [];

    // Listen for streaming output chunks
    const unlistenChunk = await listen<{ text: string }>("review-output-chunk", (event) => {
      setOutput((prev) => [...prev, event.payload.text]);
    });
    unlisteners.push(unlistenChunk);

    // Listen for completion
    const unlistenDone = await listen<{ success: boolean; message: string }>(
      "review-output-done",
      (event) => {
        if (event.payload.success) {
          setState("done");
        } else {
          setState("error");
          setError(event.payload.message);
        }
        // Clean up listeners
        unlisteners.forEach((u) => u());
      }
    );
    unlisteners.push(unlistenDone);

    try {
      await reviewPrDryRun(orgUrl, project, repo, prId);
    } catch (e: any) {
      setState("error");
      setError(String(e));
      unlisteners.forEach((u) => u());
    }
  };

  const postReview = async () => {
    setState("posting");
    setOutput([]);
    setError(null);

    const unlisteners: UnlistenFn[] = [];

    const unlistenChunk = await listen<{ text: string }>("review-post-chunk", (event) => {
      setOutput((prev) => [...prev, event.payload.text]);
    });
    unlisteners.push(unlistenChunk);

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
      await reviewPrPost(orgUrl, project, repo, prId);
    } catch (e: any) {
      setState("error");
      setError(String(e));
      unlisteners.forEach((u) => u());
    }
  };

  const handleClose = () => {
    setShowOutput(false);
    setState("idle");
    setOutput([]);
    setError(null);
  };

  return (
    <div>
      {/* Trigger button */}
      <button
        onClick={startReview}
        disabled={state === "running" || state === "posting"}
        class="px-3 py-1.5 bg-accent hover:bg-accent-hover text-white rounded-lg text-xs font-medium disabled:opacity-50 flex items-center gap-1.5"
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

      {/* Output panel */}
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
                    onClick={cancelReview}
                    class="px-2 py-1 text-xs text-red-600 dark:text-red-400 border border-red-300 dark:border-red-700 rounded hover:bg-red-50 dark:hover:bg-red-900/30"
                  >
                    Cancel
                  </button>
                )}
                {state === "done" && (
                  <button
                    onClick={postReview}
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
            <div ref={panelRef} class="flex-1 overflow-y-auto p-4 font-mono text-xs leading-relaxed">
              {error ? (
                <div class="text-red-600 dark:text-red-400 whitespace-pre-wrap">{error}</div>
              ) : output.length === 0 ? (
                <div class="text-gray-400 flex items-center gap-2">
                  <span class="animate-spin w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full" />
                  Starting review...
                </div>
              ) : (
                <div class="whitespace-pre-wrap text-gray-700 dark:text-gray-300">
                  {output.join("\n")}
                </div>
              )}

              {/* Status indicator at bottom */}
              {(state === "running" || state === "posting") && (
                <div class="sticky bottom-0 text-gray-400 pt-2">
                  <span class="animate-spin inline-block w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full mr-2" />
                  {state === "posting" ? "Posting findings to ADO..." : "Review in progress..."}
                </div>
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
