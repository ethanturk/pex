import { useState, useEffect, useRef } from "preact/hooks";
import {
  getDiffHunks,
  explainHunk,
  type DiffHunk,
} from "@/lib/api";
import { useResizableWidth } from "@/lib/useResizableWidth";

type HunkAi = { loading: boolean; text: string | null; error: string | null };

function errorMessage(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}

interface Props {
  filePath: string;
  oldContent: string;
  newContent: string;
  explainAllRequest?: { id: number; filePath: string | null };
  /// Called when the user clicks the close (×) button in the sidebar header.
  /// `PRDetail` owns the open/closed state of the sidebar.
  onClose: () => void;
}

export function HunkReview({
  filePath,
  oldContent,
  newContent,
  explainAllRequest,
  onClose,
}: Props) {
  const resize = useResizableWidth({
    storageKey: "pex-hunkreview-width",
    defaultWidth: 448,
    min: 280,
    max: 900,
    side: "left",
  });
  const [hunks, setHunks] = useState<DiffHunk[] | null>(null);
  const [explanations, setExplanations] = useState<Map<number, HunkAi>>(new Map());
  const [collapsedExplanations, setCollapsedExplanations] = useState<Set<number>>(new Set());
  const [loadError, setLoadError] = useState<string | null>(null);
  const lastExplainAllRequestId = useRef(0);

  // Load hunks once on mount. PRDetail keys this component on the file path so
  // switching files unmounts/remounts and forces a fresh load.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const result = await getDiffHunks(oldContent, newContent);
        if (!cancelled) {
          setHunks(result);
          setLoadError(null);
        }
      } catch (e) {
        if (!cancelled) setLoadError(errorMessage(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleExplainHunk = async (hunkIndex: number) => {
    setCollapsedExplanations((prev) => {
      const next = new Set(prev);
      next.delete(hunkIndex);
      return next;
    });
    setExplanations((prev) => {
      const next = new Map(prev);
      next.set(hunkIndex, { loading: true, text: null, error: null });
      return next;
    });
    try {
      const result = await explainHunk(filePath, oldContent, newContent, hunkIndex);
      setExplanations((prev) => {
        const next = new Map(prev);
        next.set(hunkIndex, { loading: false, text: result, error: null });
        return next;
      });
    } catch (e) {
      const message = errorMessage(e);
      console.error("Failed to explain hunk:", message);
      setExplanations((prev) => {
        const next = new Map(prev);
        next.set(hunkIndex, { loading: false, text: null, error: message });
        return next;
      });
    }
  };

  const toggleExplanation = (hunkIndex: number) => {
    setCollapsedExplanations((prev) => {
      const next = new Set(prev);
      if (next.has(hunkIndex)) {
        next.delete(hunkIndex);
      } else {
        next.add(hunkIndex);
      }
      return next;
    });
  };

  useEffect(() => {
    if (
      !hunks ||
      !explainAllRequest ||
      explainAllRequest.id === 0 ||
      explainAllRequest.id === lastExplainAllRequestId.current ||
      explainAllRequest.filePath !== filePath
    ) {
      return;
    }

    lastExplainAllRequestId.current = explainAllRequest.id;
    let cancelled = false;

    (async () => {
      for (const hunk of hunks) {
        if (cancelled) return;
        const explanation = explanations.get(hunk.index);
        if (explanation?.loading || explanation?.text) continue;
        await handleExplainHunk(hunk.index);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [hunks, explainAllRequest?.id, filePath]);

  return (
    <aside
      class="border-l border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 shrink-0 flex flex-col relative"
      style={{ width: `min(${resize.width}px, 100%)`, maxWidth: "100%" }}
    >
      <div
        onMouseDown={resize.onMouseDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize explain sidebar"
        title="Drag to resize"
        class="hidden sm:block absolute top-0 left-0 bottom-0 w-1.5 -ml-0.5 cursor-col-resize hover:bg-accent/40 active:bg-accent/70 z-10"
      />
      {/* Sidebar header */}
      <div class="px-4 py-2 border-b border-gray-200 dark:border-gray-700 flex items-center gap-2 shrink-0">
        <span class="text-sm font-semibold">✨ Explain</span>
        {hunks && (
          <span class="text-xs text-gray-400">
            {hunks.length} hunk{hunks.length !== 1 ? "s" : ""}
          </span>
        )}
        <button
          onClick={onClose}
          aria-label="Close explain sidebar"
          title="Close"
          class="ml-auto text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-lg leading-none px-1"
        >
          ×
        </button>
      </div>

      {/* Hunks list */}
      <div class="flex-1 overflow-y-auto">
          {loadError ? (
            <div class="p-4 text-sm text-red-600 dark:text-red-400">{loadError}</div>
          ) : !hunks ? (
            <div class="p-4 text-sm text-gray-400">Loading hunks...</div>
          ) : hunks.length === 0 ? (
            <div class="p-4 text-sm text-gray-400">No hunks to explain (file unchanged).</div>
          ) : (
            hunks.map((hunk) => {
              const explanation = explanations.get(hunk.index);
              const isExplanationCollapsed = collapsedExplanations.has(hunk.index);
              return (
                <div
                  key={hunk.index}
                  class="border-b border-gray-200 dark:border-gray-700 last:border-b-0"
                >
                  {/* Hunk header */}
                  <div class="flex items-center gap-3 px-4 py-2 bg-gray-100 dark:bg-gray-800">
                    <span class="min-w-0 flex-1 truncate text-xs font-mono text-gray-500">
                      {hunk.header}
                    </span>
                    <button
                      onClick={() => handleExplainHunk(hunk.index)}
                      disabled={explanation?.loading}
                      class="shrink-0 text-xs px-2 py-1 rounded border border-gray-300 dark:border-gray-600 hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-50"
                    >
                      {explanation?.loading
                        ? "Explaining..."
                        : explanation?.text
                          ? "Re-explain"
                          : "✨ Explain"}
                    </button>
                  </div>

                  {explanation?.error && (
                    <div class="px-4 py-2 text-xs text-red-700 dark:text-red-300 bg-red-50 dark:bg-red-900/20 border-t border-red-200 dark:border-red-800">
                      <div class="font-semibold mb-0.5">Explain failed</div>
                      <div class="break-words">{explanation.error}</div>
                    </div>
                  )}

                  {/* Explanation result */}
                  {explanation?.loading && (
                    <div class="px-4 py-2 flex items-center gap-2 text-xs text-gray-400 border-t border-gray-200 dark:border-gray-700">
                      <div class="animate-spin w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full" />
                      Explaining hunk...
                    </div>
                  )}
                  {explanation?.text && (
                    <div class="px-4 py-2 text-xs text-gray-700 dark:text-gray-300 leading-relaxed whitespace-pre-wrap border-t border-gray-200 dark:border-gray-700">
                      <div class="flex items-center gap-2 mb-1">
                        <div class="text-[10px] uppercase tracking-wide text-gray-400">
                          Explanation
                        </div>
                        <button
                          type="button"
                          onClick={() => toggleExplanation(hunk.index)}
                          aria-expanded={!isExplanationCollapsed}
                          aria-label={
                            isExplanationCollapsed
                              ? "Expand explanation"
                              : "Collapse explanation"
                          }
                          title={
                            isExplanationCollapsed
                              ? "Expand explanation"
                              : "Collapse explanation"
                          }
                          class="inline-flex items-center justify-center w-5 h-5 rounded border border-gray-300 dark:border-gray-600 text-xs leading-none text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-700"
                        >
                          {isExplanationCollapsed ? "+" : "-"}
                        </button>
                      </div>
                      {!isExplanationCollapsed && explanation.text}
                    </div>
                  )}

                  {/* Mini diff */}
                  <div class="font-mono text-[12px] leading-5 overflow-x-auto">
                    <div class="inline-block min-w-full">
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
                          <div class={`flex min-w-full w-max ${bgClass}`} key={i}>
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
                  </div>
                </div>
              );
            })
          )}
        </div>
    </aside>
  );
}
