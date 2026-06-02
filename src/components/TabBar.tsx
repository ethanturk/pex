import {
  openTabs,
  previewPath,
  activeTab,
  prFiles,
  reviewRuns,
  activeReviewPrId,
  PR_SUMMARY_TAB,
  PR_REVIEW_TAB,
  pinTab,
  closeTab,
  focusPrSummaryTab,
  focusPrReviewTab,
  selectedFile,
} from "@/lib/signals";
import { STATUS_ICON, STATUS_COLOR } from "@/lib/fileStatus";

interface Props {
  prId: number;
}

function basename(path: string): string {
  const i = path.lastIndexOf("/");
  return i >= 0 ? path.slice(i + 1) : path;
}

// Mirrors the status label/spinner the old header "Review PR" button showed,
// now surfaced on the permanent PR Review tab.
function reviewTabState(prId: number) {
  const run = reviewRuns.value.get(prId);
  const isThisRunning = activeReviewPrId.value === prId;
  if (isThisRunning) return { label: "Reviewing…", spinner: true };
  if (run?.status === "posting") return { label: "Posting…", spinner: true };
  if (run?.status === "done") return { label: `🔍 PR Review (${run.output?.findings.length ?? 0})`, spinner: false };
  if (run?.status === "posted") return { label: "🔍 PR Review ✓", spinner: false };
  if (run?.status === "error") return { label: "🔍 PR Review (error)", spinner: false };
  return { label: "🔍 PR Review", spinner: false };
}

const TAB_BASE =
  "group flex items-center gap-1.5 shrink-0 px-3 h-8 text-xs border-r border-gray-200 dark:border-gray-800 cursor-pointer select-none max-w-[16rem]";
const TAB_ACTIVE = "bg-white dark:bg-gray-950 text-gray-900 dark:text-gray-100 border-b-2 border-b-accent";
const TAB_INACTIVE =
  "bg-gray-50 dark:bg-gray-900 text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800";

function FileTab({ path, preview }: { path: string; preview: boolean }) {
  const isActive = activeTab.value === path;
  const file = prFiles.value.find((f) => f.path === path);
  const status = file?.status ?? "";
  return (
    <div
      class={`${TAB_BASE} ${isActive ? TAB_ACTIVE : TAB_INACTIVE}`}
      title={path}
      onClick={() => {
        activeTab.value = path;
        selectedFile.value = path;
      }}
      onDblClick={() => pinTab(path)}
    >
      <span class={`font-mono text-[11px] w-3 text-center shrink-0 ${STATUS_COLOR[status] || ""}`}>
        {STATUS_ICON[status] || ""}
      </span>
      <span class={`truncate ${preview ? "italic" : ""}`}>{basename(path)}</span>
      <button
        class="shrink-0 w-4 h-4 flex items-center justify-center rounded text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 hover:bg-gray-200 dark:hover:bg-gray-700 leading-none"
        title="Close tab"
        aria-label={`Close ${basename(path)}`}
        onClick={(e) => {
          e.stopPropagation();
          closeTab(path);
        }}
      >
        ×
      </button>
    </div>
  );
}

export function TabBar({ prId }: Props) {
  const review = reviewTabState(prId);
  const summaryActive = activeTab.value === PR_SUMMARY_TAB;
  const reviewActive = activeTab.value === PR_REVIEW_TAB;
  const pinned = openTabs.value;
  const preview = previewPath.value;
  const showPreview = preview && !pinned.includes(preview);

  return (
    <div class="flex items-stretch overflow-x-auto whitespace-nowrap border-b border-gray-200 dark:border-gray-800 shrink-0 bg-gray-50 dark:bg-gray-900">
      <div
        class={`${TAB_BASE} font-medium ${summaryActive ? TAB_ACTIVE : TAB_INACTIVE}`}
        title="PR summary"
        onClick={focusPrSummaryTab}
      >
        <span class="truncate">Summary</span>
      </div>

      <div
        class={`${TAB_BASE} font-medium ${reviewActive ? TAB_ACTIVE : TAB_INACTIVE}`}
        title="PR review"
        onClick={focusPrReviewTab}
      >
        {review.spinner && (
          <span class="animate-spin w-3 h-3 border-2 border-current border-t-transparent rounded-full shrink-0" />
        )}
        <span class="truncate">{review.label}</span>
      </div>

      {pinned.map((path) => (
        <FileTab key={path} path={path} preview={false} />
      ))}
      {showPreview && <FileTab key={preview} path={preview} preview={true} />}
    </div>
  );
}
