import { useEffect, useRef, useState } from "preact/hooks";
import { marked } from "marked";
import {
  reviewRuns,
  activeReviewPrId,
  sidebarMode,
  selectedFile,
  pendingScrollLine,
  threadsRefreshTick,
  type PRReviewRun,
  type ReviewProgress,
} from "@/lib/signals";
import { startBackgroundReview } from "@/lib/reviewBus";
import { cancelReview, postReviewFinding, type Severity } from "@/lib/api";
import { useResizableWidth } from "@/lib/useResizableWidth";

type Finding = NonNullable<PRReviewRun["output"]>["findings"][number];

const SEVERITY_ORDER: Severity[] = ["critical", "moderate", "minor"];

function severityBadgeClass(s: Severity): string {
  switch (s) {
    case "critical": return "bg-red-500";
    case "moderate": return "bg-amber-500";
    case "minor":    return "bg-gray-400";
  }
}

function severityLabel(s: Severity): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function fileName(path: string): string {
  const i = path.lastIndexOf("/");
  return i >= 0 ? path.slice(i + 1) : path;
}

function lineSuffix(f: Finding): string {
  if (f.lineStart == null) return "";
  if (f.lineEnd == null || f.lineEnd === f.lineStart) return `:${f.lineStart}`;
  return `:${f.lineStart}-${f.lineEnd}`;
}

function renderMarkdown(markdown: string): string {
  return marked.parse(markdown, {
    async: false,
    breaks: true,
    gfm: true,
  }) as string;
}

function stripStatisticsSection(markdown: string): string {
  const idx = markdown.search(/^##\s+Statistics\s*$/im);
  return idx >= 0 ? markdown.slice(0, idx).trim() : markdown;
}

function findingCounts(findings: Finding[]) {
  return {
    critical: findings.filter((f) => f.severity === "critical").length,
    moderate: findings.filter((f) => f.severity === "moderate").length,
    minor: findings.filter((f) => f.severity === "minor").length,
  };
}

interface Props {
  projectId: string;
  repoId: string;
  prId: number;
  prTitle: string;
}

function progressText(p: ReviewProgress | null): string {
  if (!p) return "Starting review...";
  switch (p.phase) {
    case "resume": return "Resuming from saved progress...";
    case "diff-fetch": return p.detail;
    case "hunk-review": return `Reviewing ${p.detail} — hunk ${p.hunk}/${p.totalHunks}`;
    case "file-aggregate":
    case "batch-aggregate": return p.detail;
    case "synthesis": return "Producing final review summary...";
    case "posting": return "Posting findings to ADO...";
    case "done": return "Review complete";
    default: return p.detail;
  }
}

function progressPercent(p: ReviewProgress | null): number {
  if (!p) return 0;
  if (p.phase === "diff-fetch" && p.totalFiles) {
    return Math.round(((p.fileNum || 0) / p.totalFiles) * 100);
  }
  if (p.phase === "hunk-review" && p.totalFiles && p.fileNum) {
    return Math.round(
      ((p.fileNum - 1 + (p.hunk || 0) / (p.totalHunks || 1)) / p.totalFiles) * 100,
    );
  }
  if (p.totalFiles && p.fileNum) {
    return Math.round((Math.min(p.fileNum, p.totalFiles) / p.totalFiles) * 100);
  }
  if (p.phase === "batch-aggregate" && p.totalBatches) {
    return Math.round(((p.batch || 0) / p.totalBatches) * 100);
  }
  return 0;
}

function progressFileCount(p: ReviewProgress | null): string {
  if (!p?.totalFiles) return "";
  const fileNum = Math.min(p.fileNum ?? 0, p.totalFiles);
  return `${fileNum}/${p.totalFiles}`;
}

export function PRReviewSidebar({ projectId, repoId, prId, prTitle }: Props) {
  const resize = useResizableWidth({
    storageKey: "pex-prreview-width",
    defaultWidth: 480,
    min: 320,
    max: 900,
    side: "left",
  });

  const run: PRReviewRun | undefined = reviewRuns.value.get(prId);
  const summaryRef = useRef<HTMLDivElement>(null);

  // Auto-scroll progress text as new updates land
  useEffect(() => {
    if (summaryRef.current) {
      summaryRef.current.scrollTop = summaryRef.current.scrollHeight;
    }
  }, [run?.progress]);

  const running = run?.status === "running";
  const posting = run?.status === "posting";
  const busyElsewhere =
    activeReviewPrId.value !== null && activeReviewPrId.value !== prId;

  const restart = () => startBackgroundReview(projectId, repoId, prId, prTitle);
  const close = () => (sidebarMode.value = null);

  // Selection + posted state lives here so the footer's "Post N to ADO"
  // button can drive a per-finding post loop over the user's selection.
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [posted, setPosted] = useState<Set<number>>(new Set());
  const [bulkPosting, setBulkPosting] = useState(false);
  const [bulkError, setBulkError] = useState<string | null>(null);

  const findings = run?.output?.findings ?? [];

  // Drop selection/posted state if the run is replaced (re-review, cancel).
  useEffect(() => {
    setSelected(new Set());
    setPosted(new Set());
    setBulkError(null);
  }, [run?.output]);

  const toggleSelected = (i: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      return next;
    });
  };

  const markPosted = (i: number) => {
    setPosted((prev) => {
      if (prev.has(i)) return prev;
      const next = new Set(prev);
      next.add(i);
      return next;
    });
    setSelected((prev) => {
      if (!prev.has(i)) return prev;
      const next = new Set(prev);
      next.delete(i);
      return next;
    });
  };

  const postSelected = async () => {
    const indices = Array.from(selected).filter((i) => !posted.has(i));
    if (indices.length === 0) return;
    setBulkPosting(true);
    setBulkError(null);
    let firstError: string | null = null;
    let anyPosted = false;
    for (const i of indices) {
      const f = findings[i];
      if (!f) continue;
      try {
        await postReviewFinding(
          projectId,
          repoId,
          prId,
          f.filePath || null,
          f.lineStart ?? null,
          f.lineEnd ?? null,
          f.comment,
        );
        markPosted(i);
        anyPosted = true;
      } catch (e) {
        if (!firstError) firstError = e instanceof Error ? e.message : String(e);
      }
    }
    if (anyPosted) {
      threadsRefreshTick.value = threadsRefreshTick.value + 1;
    }
    if (firstError) setBulkError(firstError);
    setBulkPosting(false);
  };

  const cancel = async () => {
    try {
      await cancelReview();
    } finally {
      // Drop this PR's run so the sidebar resets to the "No review yet" state.
      // The Rust engine sees the cancel flag, returns early, and the in-flight
      // startReview promise's .catch in reviewBus would otherwise leave the
      // run stuck in "error" — clearing it here is what makes the UI reset.
      const next = new Map(reviewRuns.value);
      next.delete(prId);
      reviewRuns.value = next;
      if (activeReviewPrId.value === prId) activeReviewPrId.value = null;
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
        aria-label="Resize PR review sidebar"
        title="Drag to resize"
        class="absolute top-0 left-0 bottom-0 w-1.5 -ml-0.5 cursor-col-resize hover:bg-accent/40 active:bg-accent/70 z-10"
      />

      {/* Header */}
      <div class="px-4 py-2 border-b border-gray-200 dark:border-gray-700 flex items-center gap-2 shrink-0">
        <span class="text-sm font-semibold">🔍 PR review</span>
        {run && (
          <span class="text-xs text-gray-400">
            {run.status === "running" && "running"}
            {run.status === "posting" && "posting"}
            {run.status === "done" && `${run.output?.findings.length ?? 0} findings`}
            {run.status === "posted" && "posted ✓"}
            {run.status === "error" && "error"}
          </span>
        )}
        <button
          onClick={close}
          aria-label="Close PR review sidebar"
          title="Close"
          class="ml-auto text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 text-lg leading-none px-1"
        >
          ×
        </button>
      </div>

      {/* Body */}
      <div class="flex-1 overflow-y-auto p-4 text-sm" ref={summaryRef}>
        {!run ? (
          <div class="text-gray-400">
            No review yet for this PR.
            <div class="mt-3">
              <button
                onClick={restart}
                disabled={busyElsewhere}
                title={busyElsewhere ? `Another review is running (PR #${activeReviewPrId.value})` : undefined}
                class="px-3 py-1.5 bg-accent hover:bg-accent-hover text-white rounded-lg text-xs font-medium disabled:opacity-50 disabled:cursor-not-allowed"
              >
                Start review
              </button>
            </div>
          </div>
        ) : (
          <>
            {(running || posting) && (
              <div class="mb-4 rounded border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-900 px-3 py-2">
                <div class="flex items-center gap-2">
                  <span class="animate-spin w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full shrink-0" />
                  <span class="text-xs font-semibold text-gray-700 dark:text-gray-200">
                    Review in Progress
                  </span>
                  {progressFileCount(run.progress) && (
                    <span class="ml-auto text-xs font-mono text-gray-500 dark:text-gray-400">
                      {progressFileCount(run.progress)}
                    </span>
                  )}
                  {running && (
                    <button
                      onClick={cancel}
                      class="ml-2 px-2 py-0.5 text-[11px] text-red-600 dark:text-red-400 border border-red-300 dark:border-red-700 rounded font-medium hover:bg-red-50 dark:hover:bg-red-900/30"
                    >
                      Cancel
                    </button>
                  )}
                </div>
                <div class="mt-2 bg-gray-200 dark:bg-gray-700 rounded-full h-1.5 overflow-hidden">
                  <div
                    class="bg-accent h-full rounded-full transition-all duration-300"
                    style={{ width: `${progressPercent(run.progress)}%` }}
                  />
                </div>
                <div class="mt-1 text-[11px] text-gray-500 dark:text-gray-400 truncate">
                  {progressText(run.progress)}
                </div>
              </div>
            )}

            {run.error && (
              <div class="text-red-600 dark:text-red-400 whitespace-pre-wrap mb-4">
                {run.error}
              </div>
            )}

            {run.output?.summary && (
              <>
                <MarkdownSummary markdown={stripStatisticsSection(run.output.summary)} />
                <ExactStatistics findings={run.output.findings} />
              </>
            )}

            {bulkError && (
              <div class="mt-3 text-red-600 dark:text-red-400 whitespace-pre-wrap text-xs">
                {bulkError}
              </div>
            )}

            {run.output && run.output.findings.length > 0 && (
              <FindingsList
                projectId={projectId}
                repoId={repoId}
                prId={prId}
                findings={run.output.findings}
                selected={selected}
                posted={posted}
                onToggleSelected={toggleSelected}
                onPosted={markPosted}
              />
            )}
          </>
        )}
      </div>

      {/* Footer actions */}
      {run && (
        <div class="px-4 py-2 border-t border-gray-200 dark:border-gray-700 shrink-0 flex items-center gap-2">
          {run.status === "done" && (() => {
            const selectableCount = Array.from(selected).filter((i) => !posted.has(i)).length;
            const disabled = busyElsewhere || bulkPosting || selectableCount === 0;
            const label = bulkPosting
              ? `Posting${selectableCount > 0 ? ` ${selectableCount}` : ""}...`
              : selectableCount > 0
                ? `Post ${selectableCount} finding${selectableCount === 1 ? "" : "s"} to ADO`
                : "Post findings to ADO";
            return (
              <button
                onClick={postSelected}
                disabled={disabled}
                title={selectableCount === 0 ? "Select at least one finding to enable" : undefined}
                class="px-3 py-1.5 bg-green-600 hover:bg-green-700 text-white rounded text-xs font-medium disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {label}
              </button>
            );
          })()}
          {(run.status === "done" || run.status === "error" || run.status === "posted") && (
            <button
              onClick={restart}
              disabled={busyElsewhere}
              class="px-3 py-1.5 text-xs border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
            >
              {run.status === "posted" ? "Re-review" : "Run again"}
            </button>
          )}
        </div>
      )}
    </aside>
  );
}

function MarkdownSummary({ markdown }: { markdown: string }) {
  return (
    <div
      class="pr-review-markdown text-gray-700 dark:text-gray-300 leading-relaxed"
      dangerouslySetInnerHTML={{ __html: renderMarkdown(markdown) }}
    />
  );
}

function ExactStatistics({ findings }: { findings: Finding[] }) {
  const counts = findingCounts(findings);
  return (
    <div class="pr-review-markdown text-gray-700 dark:text-gray-300 leading-relaxed mt-4">
      <h2>Statistics</h2>
      <ul>
        <li>
          Issues found: {counts.critical} critical, {counts.moderate} moderate, {counts.minor} minor
        </li>
      </ul>
    </div>
  );
}

// ---- Findings list ----

interface FindingsListProps {
  projectId: string;
  repoId: string;
  prId: number;
  findings: Finding[];
  selected: Set<number>;
  posted: Set<number>;
  onToggleSelected: (i: number) => void;
  onPosted: (i: number) => void;
}

function FindingsList({
  projectId,
  repoId,
  prId,
  findings,
  selected,
  posted,
  onToggleSelected,
  onPosted,
}: FindingsListProps) {
  const [editingIdx, setEditingIdx] = useState<number | null>(null);

  // Group by severity (critical → moderate → minor); within each severity,
  // sort by file path so related files cluster together. Each finding keeps
  // its original index so post-tracking survives the regrouping.
  const indexed = findings.map((f, i) => ({ f, i }));
  const bySeverity = new Map<Severity, { f: Finding; i: number }[]>();
  for (const entry of indexed) {
    const list = bySeverity.get(entry.f.severity) ?? [];
    list.push(entry);
    bySeverity.set(entry.f.severity, list);
  }
  for (const list of bySeverity.values()) {
    list.sort((a, b) =>
      (a.f.filePath || "").localeCompare(b.f.filePath || ""),
    );
  }
  const severityOrder = SEVERITY_ORDER.filter((s) => bySeverity.has(s));

  const handlePosted = (i: number) => {
    onPosted(i);
    setEditingIdx(null);
  };

  return (
    <div class="mt-4">
      <div class="text-[10px] uppercase tracking-wide text-gray-400 mb-2">
        Findings ({findings.length})
      </div>
      <div class="space-y-3">
        {severityOrder.map((sev) => (
          <div key={sev}>
            <div class="flex items-center gap-1.5 mb-1">
              <span class={`inline-block w-2 h-2 rounded-full ${severityBadgeClass(sev)}`} />
              <span class="text-[10px] uppercase tracking-wide text-gray-500">
                {severityLabel(sev)} ({bySeverity.get(sev)!.length})
              </span>
            </div>
            <ul class="space-y-2">
              {bySeverity.get(sev)!.map(({ f, i }) => (
                <FindingRow
                  key={i}
                  finding={f}
                  projectId={projectId}
                  repoId={repoId}
                  prId={prId}
                  isPosted={posted.has(i)}
                  isSelected={selected.has(i)}
                  onToggleSelected={() => onToggleSelected(i)}
                  isEditing={editingIdx === i}
                  onEdit={() => setEditingIdx(i)}
                  onCancel={() => setEditingIdx(null)}
                  onPosted={() => handlePosted(i)}
                />
              ))}
            </ul>
          </div>
        ))}
      </div>
    </div>
  );
}

interface FindingRowProps {
  finding: Finding;
  projectId: string;
  repoId: string;
  prId: number;
  isPosted: boolean;
  isSelected: boolean;
  onToggleSelected: () => void;
  isEditing: boolean;
  onEdit: () => void;
  onCancel: () => void;
  onPosted: () => void;
}

function FindingRow({
  finding,
  projectId,
  repoId,
  prId,
  isPosted,
  isSelected,
  onToggleSelected,
  isEditing,
  onEdit,
  onCancel,
  onPosted,
}: FindingRowProps) {
  const jumpToFinding = () => {
    if (!finding.filePath) return;
    const sameFile = selectedFile.value === finding.filePath;
    if (!sameFile) {
      selectedFile.value = finding.filePath;
    }
    pendingScrollLine.value = finding.lineStart ?? null;
    // If the file was already open, the loadDiff subscription in PRDetail
    // won't fire — but the user may have posted comments since the file was
    // first loaded, so force a refresh of the Comments pane.
    if (sameFile) {
      threadsRefreshTick.value = threadsRefreshTick.value + 1;
    }
  };
  const canJump = !!finding.filePath;
  return (
    <li class="text-xs border border-gray-200 dark:border-gray-700 rounded p-2 bg-white dark:bg-gray-900">
      <div class="flex items-start gap-2 mb-1">
        <input
          type="checkbox"
          checked={isSelected}
          disabled={isPosted}
          onChange={onToggleSelected}
          aria-label={isPosted ? "Already posted" : "Select to post"}
          title={isPosted ? "Already posted" : "Select to post"}
          class="mt-1 shrink-0 accent-accent disabled:opacity-40 disabled:cursor-not-allowed"
        />
        <span
          class={`inline-block w-2 h-2 rounded-full mt-1 shrink-0 ${severityBadgeClass(finding.severity)}`}
          title={severityLabel(finding.severity)}
        />
        <div class="flex-1 min-w-0">
          <div class="flex items-center gap-1.5 min-w-0">
            <div class="font-mono text-[11px] text-gray-700 dark:text-gray-300 truncate">
              {finding.filePath ? `${fileName(finding.filePath)}${lineSuffix(finding)}` : "(PR-level)"}
            </div>
            {Number.isFinite(finding.confidence) && (
              <span
                class="shrink-0 text-[10px] tabular-nums text-gray-400 dark:text-gray-500"
                title={`${severityLabel(finding.severity)} · ${finding.confidence}% confidence`}
              >
                {finding.confidence}%
              </span>
            )}
          </div>
          {finding.filePath && (
            <div
              class="font-mono text-[10px] text-gray-500 truncate"
              title={finding.filePath}
            >
              {finding.filePath}
            </div>
          )}
          <div
            onClick={canJump ? jumpToFinding : undefined}
            role={canJump ? "button" : undefined}
            tabIndex={canJump ? 0 : undefined}
            title={canJump ? "Jump to file" : undefined}
            onKeyDown={
              canJump
                ? (e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      jumpToFinding();
                    }
                  }
                : undefined
            }
            class={`whitespace-pre-wrap text-gray-700 dark:text-gray-300 mt-0.5 ${
              canJump ? "cursor-pointer hover:text-accent" : ""
            }`}
          >
            {finding.comment}
          </div>
        </div>
      </div>

      {!isEditing && (
        <div class="flex justify-end mt-1">
          {isPosted ? (
            <span class="text-[11px] text-green-600 dark:text-green-400">Posted ✓</span>
          ) : (
            <button
              onClick={onEdit}
              class="text-[11px] px-2 py-0.5 rounded border border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-800"
            >
              Create comment
            </button>
          )}
        </div>
      )}

      {isEditing && (
        <FindingEditor
          finding={finding}
          projectId={projectId}
          repoId={repoId}
          prId={prId}
          onCancel={onCancel}
          onPosted={onPosted}
        />
      )}
    </li>
  );
}

interface FindingEditorProps {
  finding: Finding;
  projectId: string;
  repoId: string;
  prId: number;
  onCancel: () => void;
  onPosted: () => void;
}

function FindingEditor({
  finding,
  projectId,
  repoId,
  prId,
  onCancel,
  onPosted,
}: FindingEditorProps) {
  const [text, setText] = useState(finding.comment);
  const [start, setStart] = useState<string>(finding.lineStart != null ? String(finding.lineStart) : "");
  const [end, setEnd] = useState<string>(finding.lineEnd != null ? String(finding.lineEnd) : "");
  const [posting, setPosting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handlePost = async () => {
    setPosting(true);
    setError(null);
    const lineStart = start.trim() === "" ? null : Number(start);
    const lineEnd = end.trim() === "" ? null : Number(end);
    if (lineStart != null && (!Number.isFinite(lineStart) || lineStart < 1)) {
      setError("Start line must be a positive integer.");
      setPosting(false);
      return;
    }
    if (lineEnd != null && (!Number.isFinite(lineEnd) || lineEnd < 1)) {
      setError("End line must be a positive integer.");
      setPosting(false);
      return;
    }
    // Both must be provided together, or neither (file-level / PR-level).
    if ((lineStart == null) !== (lineEnd == null)) {
      setError("Provide both start and end line, or leave both blank.");
      setPosting(false);
      return;
    }
    try {
      await postReviewFinding(
        projectId,
        repoId,
        prId,
        finding.filePath || null,
        lineStart,
        lineEnd,
        text,
      );
      // Notify PRDetail so its Comments pane re-fetches threads — without this
      // the comment lands in ADO but never appears in the diff view.
      threadsRefreshTick.value = threadsRefreshTick.value + 1;
      onPosted();
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setPosting(false);
    }
  };

  return (
    <div class="mt-2 border-t border-gray-200 dark:border-gray-700 pt-2 space-y-2">
      <textarea
        value={text}
        onInput={(e) => setText((e.target as HTMLTextAreaElement).value)}
        rows={4}
        class="w-full text-xs px-2 py-1 rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-900 font-mono"
      />
      <div class="flex items-center gap-2 text-[11px] text-gray-500">
        <label class="flex items-center gap-1">
          Start
          <input
            type="number"
            min={1}
            value={start}
            onInput={(e) => setStart((e.target as HTMLInputElement).value)}
            placeholder="—"
            class="w-16 px-1 py-0.5 rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-900"
          />
        </label>
        <label class="flex items-center gap-1">
          End
          <input
            type="number"
            min={1}
            value={end}
            onInput={(e) => setEnd((e.target as HTMLInputElement).value)}
            placeholder="—"
            class="w-16 px-1 py-0.5 rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-900"
          />
        </label>
        <span class="ml-auto text-gray-400">
          {start === "" && end === ""
            ? finding.filePath
              ? "file-level"
              : "PR-level"
            : `lines ${start || "?"}–${end || "?"}`}
        </span>
      </div>
      {error && (
        <div class="text-[11px] text-red-600 dark:text-red-400 whitespace-pre-wrap">{error}</div>
      )}
      <div class="flex items-center gap-2 justify-end">
        <button
          onClick={onCancel}
          disabled={posting}
          class="text-[11px] px-2 py-0.5 rounded border border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-800 disabled:opacity-50"
        >
          Cancel
        </button>
        <button
          onClick={handlePost}
          disabled={posting || text.trim() === ""}
          class="text-[11px] px-2 py-0.5 rounded bg-accent hover:bg-accent-hover text-white disabled:opacity-50"
        >
          {posting ? "Posting…" : "Post"}
        </button>
      </div>
    </div>
  );
}
