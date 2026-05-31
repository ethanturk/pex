import { useState, useEffect } from "preact/hooks";
import {
  getDiffHunks,
  explainHunk,
  type DiffHunk,
} from "@/lib/api";
import { useResizableWidth } from "@/lib/useResizableWidth";

type HunkAi = { loading: boolean; text: string | null; error: string | null };

interface Props {
  filePath: string;
  oldContent: string;
  newContent: string;
  /// Called when the user clicks the close (×) button in the sidebar header.
  /// `PRDetail` owns the open/closed state of the sidebar.
  onClose: () => void;
}

export function HunkReview({ filePath, oldContent, newContent, onClose }: Props) {
  const resize = useResizableWidth({
    storageKey: "pex-hunkreview-width",
    defaultWidth: 448,
    min: 280,
    max: 900,
    side: "left",
  });
  const [hunks, setHunks] = useState<DiffHunk[] | null>(null);
  const [explanations, setExplanations] = useState<Map<number, HunkAi>>(new Map());
  const [loadError, setLoadError] = useState<string | null>(null);

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
      } catch (e: any) {
        if (!cancelled) setLoadError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleExplainHunk = async (hunkIndex: number) => {
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
    } catch (e: any) {
      setExplanations((prev) => {
        const next = new Map(prev);
        next.set(hunkIndex, { loading: false, text: null, error: String(e) });
        return next;
      });
    }
  };

  return (
    <aside
      class="border-l border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 shrink-0 flex flex-col relative"
      style={{ width: `${resize.width}px` }}
    >
      <div
        onMouseDown={resize.onMouseDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize explain sidebar"
        title="Drag to resize"
        class="absolute top-0 left-0 bottom-0 w-1.5 -ml-0.5 cursor-col-resize hover:bg-accent/40 active:bg-accent/70 z-10"
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
              return (
                <div
                  key={hunk.index}
                  class="border-b border-gray-200 dark:border-gray-700 last:border-b-0"
                >
                  {/* Hunk header */}
                  <div class="flex items-center justify-between px-4 py-2 bg-gray-100 dark:bg-gray-800">
                    <span class="text-xs font-mono text-gray-500">{hunk.header}</span>
                    <button
                      onClick={() => handleExplainHunk(hunk.index)}
                      disabled={explanation?.loading}
                      class="text-xs px-2 py-1 rounded border border-gray-300 dark:border-gray-600 hover:bg-gray-200 dark:hover:bg-gray-700 disabled:opacity-50"
                    >
                      {explanation?.loading
                        ? "Explaining..."
                        : explanation?.text
                          ? "Re-explain"
                          : "✨ Explain"}
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

                  {/* Explanation result */}
                  {explanation?.loading && (
                    <div class="px-4 py-2 flex items-center gap-2 text-xs text-gray-400 border-t border-gray-200 dark:border-gray-700">
                      <div class="animate-spin w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full" />
                      Explaining hunk...
                    </div>
                  )}
                  {explanation?.error && (
                    <div class="px-4 py-2 text-xs text-red-600 dark:text-red-400 border-t border-gray-200 dark:border-gray-700">
                      {explanation.error}
                    </div>
                  )}
                  {explanation?.text && (
                    <div class="px-4 py-2 text-xs text-gray-700 dark:text-gray-300 leading-relaxed whitespace-pre-wrap border-t border-gray-200 dark:border-gray-700">
                      <div class="text-[10px] uppercase tracking-wide text-gray-400 mb-1">
                        Explanation
                      </div>
                      {explanation.text}
                    </div>
                  )}
                </div>
              );
            })
          )}
        </div>
    </aside>
  );
}
