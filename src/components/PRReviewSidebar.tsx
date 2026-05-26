import { useEffect, useRef, useState } from "preact/hooks";
import {
  reviewRuns,
  activeReviewPrId,
  sidebarMode,
  selectedFile,
  pendingScrollLine,
  type PRReviewRun,
  type ReviewProgress,
} from "@/lib/signals";
import { postBackgroundReview, startBackgroundReview } from "@/lib/reviewBus";
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

function findingAnchor(f: Finding): string {
  if (f.lineStart != null && f.lineEnd != null) {
    return f.lineStart === f.lineEnd
      ? `${f.filePath}:${f.lineStart}`
      : `${f.filePath}:${f.lineStart}-${f.lineEnd}`;
  }
  if (f.filePath) return f.filePath;
  return "(PR-level)";
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
  if (p.phase === "hunk-review" && p.totalFiles && p.fileNum) {
    return Math.round(
      ((p.fileNum - 1 + (p.hunk || 0) / (p.totalHunks || 1)) / p.totalFiles) * 100,
    );
  }
  if (p.phase === "batch-aggregate" && p.totalBatches) {
    return Math.round(((p.batch || 0) / p.totalBatches) * 100);
  }
  return 0;
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
  const post = () => postBackgroundReview(projectId, repoId, prId, prTitle);
  const close = () => (sidebarMode.value = null);

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
              <div class="mb-4">
                <div class="flex items-center gap-2 text-gray-400">
                  <span class="animate-spin w-3 h-3 border-2 border-gray-300 border-t-accent rounded-full" />
                  <span>{progressText(run.progress)}</span>
                </div>
                {run.progress && progressPercent(run.progress) > 0 && (
                  <div class="mt-2 bg-gray-200 dark:bg-gray-700 rounded-full h-1.5 overflow-hidden">
                    <div
                      class="bg-accent h-full rounded-full transition-all duration-300"
                      style={{ width: `${progressPercent(run.progress)}%` }}
                    />
                  </div>
                )}
              </div>
            )}

            {run.error && (
              <div class="text-red-600 dark:text-red-400 whitespace-pre-wrap mb-4">
                {run.error}
              </div>
            )}

            {run.output?.summary && (
              <div class="whitespace-pre-wrap text-gray-700 dark:text-gray-300 leading-relaxed">
                {run.output.summary}
              </div>
            )}

            {run.output && run.output.findings.length > 0 && (
              <FindingsList
                projectId={projectId}
                repoId={repoId}
                prId={prId}
                findings={run.output.findings}
              />
            )}
          </>
        )}
      </div>

      {/* Footer actions */}
      {run && (
        <div class="px-4 py-2 border-t border-gray-200 dark:border-gray-700 shrink-0 flex items-center gap-2">
          {(running || posting) && (
            <button
              onClick={() => cancelReview()}
              class="px-2 py-1 text-xs text-red-600 dark:text-red-400 border border-red-300 dark:border-red-700 rounded hover:bg-red-50 dark:hover:bg-red-900/30"
            >
              Cancel
            </button>
          )}
          {run.status === "done" && (
            <button
              onClick={post}
              disabled={busyElsewhere}
              class="px-3 py-1.5 bg-green-600 hover:bg-green-700 text-white rounded text-xs font-medium disabled:opacity-50 disabled:cursor-not-allowed"
            >
              Post findings to ADO
            </button>
          )}
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

// ---- Findings list ----

interface FindingsListProps {
  projectId: string;
  repoId: string;
  prId: number;
  findings: Finding[];
}

function FindingsList({ projectId, repoId, prId, findings }: FindingsListProps) {
  // Posted-finding tracker is local component state (in-memory): when the user
  // posts a finding we want a checkmark, but we don't persist it across app
  // restarts (run results aren't persisted either).
  const [posted, setPosted] = useState<Set<number>>(new Set());
  const [editingIdx, setEditingIdx] = useState<number | null>(null);

  // Group by file, preserving the engine's natural order, and sort each
  // file's findings by severity (critical → moderate → minor). Each finding
  // keeps its original index so post-tracking works across the regrouping.
  const indexed = findings.map((f, i) => ({ f, i }));
  const fileOrder: string[] = [];
  const byFile = new Map<string, { f: Finding; i: number }[]>();
  for (const entry of indexed) {
    const key = entry.f.filePath || "(PR-level)";
    if (!byFile.has(key)) {
      byFile.set(key, []);
      fileOrder.push(key);
    }
    byFile.get(key)!.push(entry);
  }
  for (const list of byFile.values()) {
    list.sort(
      (a, b) =>
        SEVERITY_ORDER.indexOf(a.f.severity) - SEVERITY_ORDER.indexOf(b.f.severity),
    );
  }

  const markPosted = (i: number) => {
    setPosted((prev) => {
      const next = new Set(prev);
      next.add(i);
      return next;
    });
    setEditingIdx(null);
  };

  return (
    <div class="mt-4">
      <div class="text-[10px] uppercase tracking-wide text-gray-400 mb-2">
        Findings ({findings.length})
      </div>
      <div class="space-y-3">
        {fileOrder.map((file) => (
          <div key={file}>
            <div class="font-mono text-[11px] text-gray-500 mb-1">{file}</div>
            <ul class="space-y-2">
              {byFile.get(file)!.map(({ f, i }) => (
                <FindingRow
                  key={i}
                  finding={f}
                  projectId={projectId}
                  repoId={repoId}
                  prId={prId}
                  isPosted={posted.has(i)}
                  isEditing={editingIdx === i}
                  onEdit={() => setEditingIdx(i)}
                  onCancel={() => setEditingIdx(null)}
                  onPosted={() => markPosted(i)}
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
  isEditing,
  onEdit,
  onCancel,
  onPosted,
}: FindingRowProps) {
  const jumpToFinding = () => {
    if (!finding.filePath) return;
    if (selectedFile.value !== finding.filePath) {
      selectedFile.value = finding.filePath;
    }
    pendingScrollLine.value = finding.lineStart ?? null;
  };
  const canJump = !!finding.filePath;
  return (
    <li class="text-xs border border-gray-200 dark:border-gray-700 rounded p-2 bg-white dark:bg-gray-900">
      <div class="flex items-start gap-2 mb-1">
        <span
          class={`inline-block w-2 h-2 rounded-full mt-1 shrink-0 ${severityBadgeClass(finding.severity)}`}
          title={severityLabel(finding.severity)}
        />
        <div class="flex-1 min-w-0">
          <div class="font-mono text-[11px] text-gray-500 truncate">
            {findingAnchor(finding)}
          </div>
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
