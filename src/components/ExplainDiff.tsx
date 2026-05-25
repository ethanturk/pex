import { useState } from "preact/hooks";
import { explainDiff } from "@/lib/api";

interface Props {
  filePath: string;
  oldContent: string;
  newContent: string;
}

export function ExplainDiff({ filePath, oldContent, newContent }: Props) {
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [explanation, setExplanation] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handleExplain = async () => {
    if (open) {
      setOpen(false);
      return;
    }

    // If we already have an explanation for this file, just toggle open
    if (explanation) {
      setOpen(true);
      return;
    }

    setOpen(true);
    setLoading(true);
    setError(null);

    try {
      const result = await explainDiff(filePath, oldContent, newContent);
      setExplanation(result);
    } catch (e: any) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div>
      {/* Toggle button */}
      <div class="px-4 py-2 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 flex items-center gap-2">
        <button
          onClick={handleExplain}
          class="text-xs px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700 font-medium"
        >
          {open ? "Hide explanation" : "✨ Explain this diff"}
        </button>
        {loading && <span class="text-xs text-gray-400">Asking AI...</span>}
      </div>

      {/* Explanation panel */}
      {open && (
        <div class="border-t border-gray-200 dark:border-gray-700 p-4 bg-gray-50 dark:bg-gray-800/50 max-h-96 overflow-y-auto">
          {loading ? (
            <div class="flex items-center gap-2 text-sm text-gray-400">
              <div class="animate-spin w-4 h-4 border-2 border-gray-300 border-t-accent rounded-full" />
              Generating explanation...
            </div>
          ) : error ? (
            <div class="text-sm text-red-600 dark:text-red-400">{error}</div>
          ) : explanation ? (
            <div class="text-sm text-gray-700 dark:text-gray-300 leading-relaxed whitespace-pre-wrap">
              {explanation}
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}
