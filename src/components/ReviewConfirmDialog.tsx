import { useEffect, useMemo, useState } from "preact/hooks";
import {
  getAiSettings,
  getReviewSpecialists,
  type ReviewMode,
  type ReviewSpecialistInfo,
} from "@/lib/api";

const SPECIALISTS_KEY = "pex.reviewSpecialists";

// Persisted set of enabled specialist keys so the user's last choice carries
// across reviews. Stored as a JSON array; unknown/missing falls back to "all".
function loadEnabledSpecialists(): string[] | null {
  try {
    const raw = localStorage.getItem(SPECIALISTS_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((k) => typeof k === "string") : null;
  } catch {
    return null;
  }
}

function saveEnabledSpecialists(keys: string[]) {
  try {
    localStorage.setItem(SPECIALISTS_KEY, JSON.stringify(keys));
  } catch {
    // Storage may be unavailable; selection still applies for this run.
  }
}

// Specialist labels come from the backend as "Multi-pass: code reviewer";
// strip the shared prefix for a tidier list in the dialog.
function shortLabel(label: string): string {
  return label.replace(/^Multi-pass:\s*/i, "");
}

interface Props {
  initialMode: ReviewMode;
  prId: number;
  prTitle: string;
  /** Whether another review is already running (disables Start). */
  busyElsewhere: boolean;
  onConfirm: (mode: ReviewMode, enabledSpecialists?: string[]) => void;
  onClose: () => void;
}

export function ReviewConfirmDialog({
  initialMode,
  prId,
  prTitle,
  busyElsewhere,
  onConfirm,
  onClose,
}: Props) {
  const [mode, setMode] = useState<ReviewMode>(initialMode);
  const [specialists, setSpecialists] = useState<ReviewSpecialistInfo[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [fastModel, setFastModel] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  // Load the AI tab's default model once (shown for Fast, and as the fallback
  // model for any specialist without an override).
  useEffect(() => {
    let cancelled = false;
    getAiSettings()
      .then((s) => !cancelled && setFastModel(s.model))
      .catch((e) => !cancelled && setError(e instanceof Error ? e.message : String(e)));
    return () => {
      cancelled = true;
    };
  }, []);

  // Lazily load the specialist roster the first time Thorough is selected.
  useEffect(() => {
    if (mode !== "thorough" || specialists != null) return;
    let cancelled = false;
    setLoading(true);
    getReviewSpecialists()
      .then((list) => {
        if (cancelled) return;
        const available = new Set(list.map((s) => s.key));
        const stored = loadEnabledSpecialists();
        let initial = stored ? stored.filter((k) => available.has(k)) : list.map((s) => s.key);
        if (initial.length === 0) initial = list.map((s) => s.key);
        setSpecialists(list);
        setSelected(new Set(initial));
      })
      .catch((e) => !cancelled && setError(e instanceof Error ? e.message : String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [mode, specialists]);

  const allSelected = useMemo(
    () => specialists != null && specialists.length > 0 && selected.size === specialists.length,
    [specialists, selected],
  );

  const toggle = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const toggleAll = () => {
    if (!specialists) return;
    setSelected(allSelected ? new Set() : new Set(specialists.map((s) => s.key)));
  };

  const thoroughReady = mode === "thorough" && specialists != null;
  const canStart =
    !busyElsewhere &&
    !error &&
    (mode === "fast" || (thoroughReady && selected.size > 0));

  const start = () => {
    if (!canStart) return;
    if (mode === "thorough" && specialists) {
      const keys = specialists.map((s) => s.key).filter((k) => selected.has(k));
      saveEnabledSpecialists(keys);
      // Pass undefined when every specialist is selected so the backend runs its
      // full roster (and stays forward-compatible if the roster grows).
      onConfirm("thorough", keys.length === specialists.length ? undefined : keys);
    } else {
      onConfirm("fast", undefined);
    }
  };

  return (
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        class="bg-white dark:bg-gray-900 border border-gray-200 dark:border-gray-700 rounded-lg shadow-xl w-[460px] max-w-[92vw] max-h-[85vh] flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div class="px-5 pt-4 pb-3 border-b border-gray-200 dark:border-gray-700">
          <div class="text-sm font-semibold text-gray-900 dark:text-gray-100">Start review</div>
          <div class="mt-1 text-xs text-gray-500 dark:text-gray-400 truncate">
            PR #{prId} — {prTitle}
          </div>

          {/* Fast / Thorough selector */}
          <div class="mt-3 inline-flex rounded-lg border border-gray-200 dark:border-gray-700 p-0.5 bg-gray-50 dark:bg-gray-800">
            {(["fast", "thorough"] as ReviewMode[]).map((m) => (
              <button
                key={m}
                onClick={() => setMode(m)}
                class={`px-3 py-1 text-xs font-medium rounded-md capitalize ${
                  mode === m
                    ? "bg-white dark:bg-gray-900 text-accent shadow-sm"
                    : "text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200"
                }`}
              >
                {m}
              </button>
            ))}
          </div>
          <div class="mt-1.5 text-[11px] text-gray-400">
            {mode === "thorough"
              ? "Multiple specialist agents per hunk — slower, broader coverage."
              : "A single generalist pass per hunk, with a lite design-principles check — fast."}
          </div>
        </div>

        {/* Body */}
        <div class="px-5 py-4 overflow-y-auto text-sm">
          {error ? (
            <div class="text-red-600 dark:text-red-400 whitespace-pre-wrap text-xs">{error}</div>
          ) : mode === "fast" ? (
            <div class="text-gray-600 dark:text-gray-300">
              <p class="text-xs text-gray-500 dark:text-gray-400 mb-3">
                One pass per hunk covering bugs, logic and edge cases, and security,
                plus a lite design-principles (DRY/SOLID) check.
              </p>
              <div class="flex items-center gap-2 text-xs">
                <span class="text-gray-500 dark:text-gray-400">Model</span>
                <span class="font-mono px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-800 text-gray-700 dark:text-gray-200">
                  {fastModel ?? "loading…"}
                </span>
              </div>
            </div>
          ) : loading || !specialists ? (
            <div class="flex items-center gap-2 text-gray-500 dark:text-gray-400 text-xs">
              <span class="animate-spin w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full" />
              Loading agents…
            </div>
          ) : (
            <>
              <div class="flex items-center justify-between mb-2">
                <span class="text-xs text-gray-500 dark:text-gray-400">
                  Choose which specialist agents to run ({selected.size}/{specialists.length})
                </span>
                <button onClick={toggleAll} class="text-xs text-accent hover:underline">
                  {allSelected ? "Deselect all" : "Select all"}
                </button>
              </div>
              <div class="flex flex-col gap-1">
                {specialists.map((s) => {
                  const on = selected.has(s.key);
                  return (
                    <label
                      key={s.key}
                      class={`flex items-start gap-2.5 px-2.5 py-2 rounded border cursor-pointer ${
                        on
                          ? "border-accent/60 bg-accent/5"
                          : "border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800/50"
                      }`}
                    >
                      <input
                        type="checkbox"
                        checked={on}
                        onChange={() => toggle(s.key)}
                        class="mt-0.5 shrink-0 accent-accent"
                      />
                      <span class="min-w-0 flex-1">
                        <span class="flex items-center gap-2 flex-wrap">
                          <span class="font-medium text-gray-800 dark:text-gray-100">
                            {shortLabel(s.label)}
                          </span>
                          <span
                            title="Provider and model this agent will use"
                            class="font-mono text-[10px] px-1.5 py-0.5 rounded bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-300"
                          >
                            {s.providerName}: {s.model}
                          </span>
                        </span>
                        <span class="block mt-0.5 text-[11px] text-gray-500 dark:text-gray-400 leading-snug">
                          {s.description.replace(/^Thorough PR review specialist — /i, "")}
                        </span>
                      </span>
                    </label>
                  );
                })}
              </div>
              {selected.size === 0 && (
                <div class="mt-2 text-[11px] text-amber-600 dark:text-amber-400">
                  Select at least one agent to start.
                </div>
              )}
            </>
          )}
        </div>

        {/* Footer */}
        <div class="px-5 py-3 border-t border-gray-200 dark:border-gray-700 flex justify-end gap-2">
          <button
            onClick={onClose}
            class="px-3 py-1.5 rounded text-xs font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800"
          >
            Cancel
          </button>
          <button
            onClick={start}
            disabled={!canStart}
            autofocus
            title={busyElsewhere ? "Another review is already running" : undefined}
            class="px-3 py-1.5 rounded text-xs font-medium bg-accent hover:bg-accent-hover text-white disabled:opacity-50 disabled:cursor-not-allowed"
          >
            Start review
          </button>
        </div>
      </div>
    </div>
  );
}
