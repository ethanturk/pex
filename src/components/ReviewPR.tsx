import { useEffect, useState } from "preact/hooks";
import {
  reviewRuns,
  activeReviewPrId,
  sidebarMode,
} from "@/lib/signals";
import { startBackgroundReview } from "@/lib/reviewBus";
import { getSavedReview, clearSavedReview } from "@/lib/api";

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

  const [hasSavedState, setHasSavedState] = useState(false);
  useEffect(() => {
    getSavedReview().then((s) => setHasSavedState(!!s));
  }, []);

  const handleClick = () => {
    sidebarMode.value = open ? null : "pr-review";
    if (!open && !run && !busyElsewhere) {
      startBackgroundReview(projectId, repoId, prId, prTitle);
      setHasSavedState(false);
    }
  };

  const handleResume = () => {
    setHasSavedState(false);
    sidebarMode.value = "pr-review";
    if (!busyElsewhere) startBackgroundReview(projectId, repoId, prId, prTitle);
  };

  const handleDiscard = async () => {
    await clearSavedReview();
    setHasSavedState(false);
  };

  const label = (() => {
    if (isThisRunning) return "Reviewing...";
    if (run?.status === "posting") return "Posting...";
    if (run?.status === "done") return `🔍 Review (${run.output?.findings.length ?? 0})`;
    if (run?.status === "posted") return "🔍 Review ✓";
    if (run?.status === "error") return "🔍 Review (error)";
    return "🔍 Review PR";
  })();

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
              : "Open PR review sidebar"
        }
        class={`px-3 py-1.5 rounded-lg text-xs font-medium flex items-center gap-1.5 border disabled:opacity-50 disabled:cursor-not-allowed ${
          open
            ? "border-accent text-accent bg-accent/10"
            : "bg-accent hover:bg-accent-hover text-white border-transparent"
        }`}
      >
        {(isThisRunning || run?.status === "posting") && (
          <span class="animate-spin w-3 h-3 border-2 border-current/30 border-t-current rounded-full" />
        )}
        {label}
      </button>

      {hasSavedState && !run && !isThisRunning && (
        <span class="text-xs text-amber-500">
          Saved progress —
          <button onClick={handleResume} class="underline ml-1">resume</button>
          {" · "}
          <button onClick={handleDiscard} class="underline">discard</button>
        </span>
      )}
    </div>
  );
}
