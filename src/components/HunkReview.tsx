import { useState } from "preact/hooks";
import { getDiffHunks, reviewHunk, type DiffHunk } from "@/lib/api";

interface Props {
  filePath: string;
  oldContent: string;
  newContent: string;
}

export function HunkReview({ filePath, oldContent, newContent }: Props) {
  const [open, setOpen] = useState(false);
  const [hunks, setHunks] = useState<DiffHunk[] | null>(null);
  const [reviews, setReviews] = useState<Map<number, { loading: boolean; text: string | null; error: string | null }>>(new Map());
  const [loadError, setLoadError] = useState<string | null>(null);

  const handleToggle = async () => {
    if (open) {
      setOpen(false);
      return;
    }

    setOpen(true);

    // Load hunks if not already loaded
    if (!hunks) {
      try {
        const result = await getDiffHunks(oldContent, newContent);
        setHunks(result);
        setLoadError(null);
      } catch (e: any) {
        setLoadError(String(e));
      }
    }
  };

  const handleReviewHunk = async (hunkIndex: number) => {
    setReviews((prev) => {
      const next = new Map(prev);
      next.set(hunkIndex, { loading: true, text: null, error: null });
      return next;
    });

    try {
      const result = await reviewHunk(filePath, oldContent, newContent, hunkIndex);
      setReviews((prev) => {
        const next = new Map(prev);
        next.set(hunkIndex, { loading: false, text: result, error: null });
        return next;
      });
    } catch (e: any) {
      setReviews((prev) => {
        const next = new Map(prev);
        next.set(hunkIndex, { loading: false, text: null, error: String(e) });
        return next;
      });
    }
  };

  return (
    <div>
      {/* Toggle button */}
      <div class="px-4 py-2 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 flex items-center gap-2">
        <button
          onClick={handleToggle}
          class="text-xs px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700 font-medium"
        >
          {open ? "Hide hunk reviews" : "🔍 Review hunks"}
        </button>
        {hunks && (
          <span class="text-xs text-gray-400">{hunks.length} hunk{hunks.length !== 1 ? "s" : ""}</span>
        )}
      </div>

      {/* Hunks panel */}
      {open && (
        <div class="border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 max-h-[50vh] overflow-y-auto">
          {loadError ? (
            <div class="p-4 text-sm text-red-600 dark:text-red-400">{loadError}</div>
          ) : !hunks ? (
            <div class="p-4 text-sm text-gray-400">Loading hunks...</div>
          ) : hunks.length === 0 ? (
            <div class="p-4 text-sm text-gray-400">No hunks to review (file unchanged).</div>
          ) : (
            hunks.map((hunk) => {
              const review = reviews.get(hunk.index);
              return (
                <div
                  key={hunk.index}
                  class="border-b border-gray-200 dark:border-gray-700 last:border-b-0"
                >
                  {/* Hunk header */}
                  <div class="flex items-center justify-between px-4 py-2 bg-gray-100 dark:bg-gray-800">
                    <span class="text-xs font-mono text-gray-500">{hunk.header}</span>
                    <button
                      onClick={() => handleReviewHunk(hunk.index)}
                      disabled={review?.loading}
                      class="text-xs px-2 py-1 rounded border border-gray-300 dark:border-gray-600 hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-50"
                    >
                      {review?.loading ? "Reviewing..." : review?.text ? "Re-review" : "Review"}
                    </button>
                  </div>

                  {/* Mini diff */}
                  <div class="font-mono text-[12px] leading-5 overflow-x-auto">
                    {hunk.lines.map((line, i) => {
                      const bgClass =
                        line.kind === "+"
                          ? "bg-green-50 dark:bg-green-900/20"
                          : line.kind === "-"
                            ? "bg-red-50 dark:bg-red-900/20"
                            : "";
                      const textClass =
                        line.kind === "+"
                          ? "text-green-800 dark:text-green-300"
                          : line.kind === "-"
                            ? "text-red-800 dark:text-red-300"
                            : "text-gray-500";
                      return (
                        <div class={`flex ${bgClass}`} key={i}>
                          <span class="w-6 text-right pr-2 select-none text-gray-400 shrink-0">
                            {line.newLineno ?? " "}
                          </span>
                          <span class={`w-4 text-center select-none shrink-0 ${textClass}`}>
                            {line.kind}
                          </span>
                          <span class={`pl-1 whitespace-pre ${textClass}`}>
                            {line.content}
                          </span>
                        </div>
                      );
                    })}
                  </div>

                  {/* Review result */}
                  {review?.loading && (
                    <div class="px-4 py-2 flex items-center gap-2 text-xs text-gray-400">
                      <div class="animate-spin w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full" />
                      Reviewing hunk...
                    </div>
                  )}
                  {review?.error && (
                    <div class="px-4 py-2 text-xs text-red-600 dark:text-red-400">
                      {review.error}
                    </div>
                  )}
                  {review?.text && (
                    <div class="px-4 py-2 text-xs text-gray-700 dark:text-gray-300 leading-relaxed whitespace-pre-wrap">
                      {review.text}
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
