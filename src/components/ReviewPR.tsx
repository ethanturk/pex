import { useEffect, useState } from "preact/hooks";
import {
  reviewRuns,
  activeReviewPrId,
  sidebarMode,
  currentView,
  selectedProject,
  selectedRepo,
  type ReviewMode,
} from "@/lib/signals";
import { startBackgroundReview } from "@/lib/reviewBus";
import { getSavedReview, clearSavedReview, type ReviewState } from "@/lib/api";

const REVIEW_MODE_KEY = "pex.reviewMode";

function loadReviewMode(): ReviewMode {
  try {
    const v = localStorage.getItem(REVIEW_MODE_KEY);
    return v === "thorough" ? "thorough" : "fast";
  } catch {
    return "fast";
  }
}

function saveReviewMode(mode: ReviewMode) {
  try {
    localStorage.setItem(REVIEW_MODE_KEY, mode);
  } catch {
    // Storage may be unavailable (private mode, quota). The picker still
    // works in-memory for the current session — surfacing an error would
    // be more noise than signal.
  }
}

// pr_key format from Rust: `{org_url}/{project_id}/{repo_id}/{pr_id}`.
// org_url contains slashes, so split from the end.
function parsePrKey(prKey: string): { projectId: string; repoId: string; prId: number } | null {
  const parts = prKey.split("/");
  if (parts.length < 4) return null;
  const prId = Number(parts[parts.length - 1]);
  if (!Number.isFinite(prId)) return null;
  return {
    repoId: parts[parts.length - 2],
    projectId: parts[parts.length - 3],
    prId,
  };
}

function progressPercent(s: ReviewState): number | null {
  const total = s.filePaths.length;
  if (total === 0) return null;
  const done = Math.min(s.currentFileIdx, total);
  return Math.round((done / total) * 100);
}

interface Props {
  projectId: string;
  repoId: string;
  prId: number;
  prTitle: string;
}

// Stateless trigger: opens the PR review sidebar; if no run exists for this PR
// yet, also kicks off a background review. All progress and results live in
// the sidebar so the user can navigate away while it runs.
export function ReviewPR({ projectId, repoId, prId, prTitle }: Props) {
  const run = reviewRuns.value.get(prId);
  const runningPrId = activeReviewPrId.value;
  const isThisRunning = runningPrId === prId;
  const busyElsewhere = runningPrId !== null && !isThisRunning;
  const open = sidebarMode.value === "pr-review";

  const [savedReview, setSavedReview] = useState<ReviewState | null>(null);
  const [mode, setMode] = useState<ReviewMode>(() => loadReviewMode());
  useEffect(() => {
    getSavedReview().then(setSavedReview);
  }, []);

  const savedTarget = savedReview ? parsePrKey(savedReview.prKey) : null;

  const handleModeChange = (next: ReviewMode) => {
    setMode(next);
    saveReviewMode(next);
  };

  const handleClick = () => {
    sidebarMode.value = open ? null : "pr-review";
    if (!open && !run && !busyElsewhere) {
      startBackgroundReview(projectId, repoId, prId, prTitle, false, mode);
      setSavedReview(null);
    }
  };

  const handleResume = () => {
    if (busyElsewhere) return;
    const target = savedTarget;
    const resumeMode = (savedReview?.mode as ReviewMode | undefined) ?? mode;
    setSavedReview(null);
    sidebarMode.value = "pr-review";
    if (target && target.prId !== prId) {
      // Saved progress belongs to a different PR — navigate there so the
      // user can see what's resuming, then kick off the engine which will
      // pick up the saved state (engine matches on pr_key).
      selectedProject.value = target.projectId;
      selectedRepo.value = target.repoId;
      currentView.value = { kind: "pr-detail", prId: target.prId };
      startBackgroundReview(target.projectId, target.repoId, target.prId, `PR #${target.prId}`, true, resumeMode);
    } else {
      startBackgroundReview(projectId, repoId, prId, prTitle, true, resumeMode);
    }
  };

  const handleDiscard = async () => {
    await clearSavedReview();
    setSavedReview(null);
  };

  const label = (() => {
    if (isThisRunning) return "Reviewing...";
    if (run?.status === "posting") return "Posting...";
    if (run?.status === "done") return `🔍 Review (${run.output?.findings.length ?? 0})`;
    if (run?.status === "posted") return "🔍 Review ✓";
    if (run?.status === "error") return "🔍 Review (error)";
    return "🔍 Review PR";
  })();

  // Disabling the picker once a run is in flight prevents the displayed mode
  // from drifting out of sync with the run that's actually executing.
  const pickerDisabled = isThisRunning || run?.status === "posting";

  return (
    <div class="flex items-center gap-2">
      <button
        onClick={handleClick}
        disabled={busyElsewhere}
        aria-pressed={open}
        title={
          busyElsewhere
            ? `Another review is running (PR #${runningPrId})`
            : open
              ? "Close PR review sidebar"
              : `Open PR review sidebar — ${mode === "thorough" ? "Thorough (multi-pass)" : "Fast (one-shot)"}`
        }
        class={`px-3 py-1 rounded text-xs font-medium flex items-center gap-1.5 border disabled:opacity-50 disabled:cursor-not-allowed ${
          open
            ? "border-accent text-accent bg-accent/10"
            : "bg-accent hover:bg-accent-hover text-white border-transparent"
        }`}
      >
        {(isThisRunning || run?.status === "posting") && (
          <span class="animate-spin w-3.5 h-3.5 border-2 border-current border-t-transparent rounded-full" />
        )}
        {label}
      </button>

      <select
        value={mode}
        disabled={pickerDisabled || busyElsewhere}
        onChange={(e) => handleModeChange((e.currentTarget.value as ReviewMode))}
        title="Review strategy. Thorough runs multiple specialist passes per hunk (slower)."
        class="text-xs px-1.5 py-1.5 rounded-lg border bg-bg-surface border-border disabled:opacity-50 disabled:cursor-not-allowed"
      >
        <option value="fast">Fast</option>
        <option value="thorough">Thorough</option>
      </select>

      {savedReview && !run && !isThisRunning && (() => {
        const savedPrId = savedTarget?.prId;
        const savedLabel = savedPrId != null ? `PR #${savedPrId}` : "another PR";
        const pct = progressPercent(savedReview);
        const pctSuffix = pct != null ? ` (${pct}%)` : "";
        const resumeTitle = savedPrId != null && savedPrId !== prId
          ? `Jump to PR #${savedPrId} and resume its review`
          : "Resume the saved review";
        return (
          <span class="text-xs text-amber-500">
            Saved progress for {savedLabel}{pctSuffix} —
            <button
              onClick={handleResume}
              disabled={busyElsewhere}
              title={resumeTitle}
              class="underline ml-1 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              resume
            </button>
            {" · "}
            <button onClick={handleDiscard} class="underline">discard</button>
          </span>
        );
      })()}
    </div>
  );
}
