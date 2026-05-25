import { useState, useEffect, useCallback } from "preact/hooks";
import { useResizableWidth } from "@/lib/useResizableWidth";
import { currentView, prFiles, selectedFile, currentIteration, selectedProject, selectedRepo, activeOrg, diffView, visibleFilePaths, sidebarMode } from "@/lib/signals";
import {
  getPrFiles,
  getViewedFiles,
  markFileViewed,
  getFileDiff,
  updateReviewerStatus,
  getThreads,
  postComment,
  getIterations,
  type CommentThread,
} from "@/lib/api";
import { FileTree } from "@/components/FileTree";
import { DiffViewer } from "@/components/DiffViewer";
import { HunkReview } from "@/components/HunkReview";
import { PRReviewSidebar } from "@/components/PRReviewSidebar";
import { ReviewPR } from "@/components/ReviewPR";
import { ApprovalBar } from "@/components/ApprovalBar";

interface Props { prId: number; }

export function PRDetail({ prId }: Props) {
  const [diffHtml, setDiffHtml] = useState<string>("");
  const [diffPath, setDiffPath] = useState<string>("");
  const [sourceCommit, setSourceCommit] = useState<string>("");
  const [baseCommit, setBaseCommit] = useState<string | null>(null);
  const [oldContent, setOldContent] = useState<string>("");
  const [newContent, setNewContent] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [iterationCount, setIterationCount] = useState(1);
  const [threads, setThreads] = useState<CommentThread[]>([]);
  const fileTreeResize = useResizableWidth({
    storageKey: "pex-filetree-width",
    defaultWidth: 256,
    min: 160,
    max: 600,
    side: "right",
  });

  const projectId = selectedProject.value;
  const repoId = selectedRepo.value;

  // Fetch iterations on mount
  useEffect(() => {
    if (projectId && repoId) {
      getIterations(projectId, repoId, prId)
        .then((iters) => {
          if (iters.length > 0) setIterationCount(iters.length);
        })
        .catch(() => {}); // Silently fall back to 1
    }
  }, [projectId, repoId, prId]);

  const loadFiles = useCallback(async () => {
    if (!projectId || !repoId) return;
    setLoading(true);
    try {
      const files = await getPrFiles(projectId, repoId, prId, currentIteration.value);
      const viewed = await getViewedFiles(projectId, repoId, prId);
      const viewedSet = new Set(viewed);
      prFiles.value = files.map((f) => ({ ...f, viewed: viewedSet.has(f.path) }));
    } catch (e) {
      console.error("Failed to load PR files:", e);
      prFiles.value = [];
    } finally {
      setLoading(false);
    }
  }, [projectId, repoId, prId]);

  const loadDiff = useCallback(async (path: string) => {
    if (!projectId || !repoId) return;
    setLoading(true);
    try {
      const d = await getFileDiff(projectId, repoId, prId, path, currentIteration.value, diffView.value);
      setDiffHtml(d.html);
      setDiffPath(d.path);
      setSourceCommit(d.sourceCommit);
      setBaseCommit(d.baseCommit);
      setOldContent(d.oldContent);
      setNewContent(d.newContent);
      // Load threads for this file
      const allThreads = await getThreads(projectId, repoId, prId);
      setThreads(allThreads.filter((t: any) => t.filePath === d.path));
    } catch (e: any) {
      const msg = typeof e === "string" ? e : e?.message ?? String(e);
      console.error("Failed to load file diff:", e);
      setDiffPath(path);
      setDiffHtml(
        `<div class="p-4 text-sm text-red-600 dark:text-red-400 break-words">Failed to load diff: ${msg
          .replace(/&/g, "&amp;")
          .replace(/</g, "&lt;")
          .replace(/>/g, "&gt;")}</div>`
      );
      setThreads([]);
    } finally {
      setLoading(false);
    }
  }, [projectId, repoId, prId]);

  useEffect(() => { loadFiles(); }, [loadFiles]);

  useEffect(() => {
    const unsub1 = selectedFile.subscribe((path) => {
      if (path) loadDiff(path);
    });
    const unsub2 = currentIteration.subscribe(() => {
      loadFiles();
      if (selectedFile.value) loadDiff(selectedFile.value);
    });
    const unsub3 = diffView.subscribe(() => {
      if (selectedFile.value) loadDiff(selectedFile.value);
    });
    return () => { unsub1(); unsub2(); unsub3(); };
  }, [loadDiff, loadFiles]);

  const handleToggleViewed = async (path: string, viewed: boolean) => {
    if (!projectId || !repoId) return;
    await markFileViewed(projectId, repoId, prId, path, viewed);
    prFiles.value = prFiles.value.map((f) => (f.path === path ? { ...f, viewed } : f));
  };

  const handleApprove = async (vote: number) => {
    await updateReviewerStatus(projectId, repoId, prId, vote);
    currentView.value = { kind: "pr-list" };
  };

  const handlePostComment = async (
    filePath: string,
    lineStart: number,
    lineEnd: number,
    content: string,
  ) => {
    const thread = await postComment(
      projectId,
      repoId,
      prId,
      filePath,
      lineStart,
      lineEnd,
      content,
    );
    setThreads([...threads, thread]);
  };

  // ---- Keyboard shortcuts ----
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      const key = e.key.toLowerCase();

      if (key === "j") {
        e.preventDefault();
        const order = visibleFilePaths.value;
        if (order.length === 0) return;
        const idx = order.indexOf(selectedFile.value ?? "");
        const next = idx < 0 ? order[0] : idx < order.length - 1 ? order[idx + 1] : null;
        if (next) selectedFile.value = next;
      } else if (key === "k") {
        e.preventDefault();
        const order = visibleFilePaths.value;
        if (order.length === 0) return;
        const idx = order.indexOf(selectedFile.value ?? "");
        const prev = idx < 0 ? order[order.length - 1] : idx > 0 ? order[idx - 1] : null;
        if (prev) selectedFile.value = prev;
      } else if (key === "v") {
        e.preventDefault();
        const file = prFiles.value.find((f) => f.path === selectedFile.value);
        if (file) handleToggleViewed(file.path, !file.viewed);
      } else if (key === "a") {
        e.preventDefault();
        handleApprove(10);
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [prId, projectId, repoId]);

  return (
    <div class="flex flex-col h-full">
      {/* PR Header */}
      <div class="flex items-center justify-between px-4 py-2 border-b border-gray-200 dark:border-gray-800 shrink-0">
        <div class="flex items-center gap-3">
          <button
            class="text-sm text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
            onClick={() => (currentView.value = { kind: "pr-list" })}
          >
            ← Back
          </button>
          <span class="font-semibold text-sm">PR #{prId}</span>
          {iterationCount > 1 && (
            <select
              value={currentIteration.value}
              onChange={(e) => (currentIteration.value = Number(e.currentTarget.value))}
              class="text-xs px-2 py-1 rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800"
            >
              {Array.from({ length: iterationCount }, (_, i) => i + 1).map((n) => (
                <option key={n} value={n}>Iteration {n}</option>
              ))}
            </select>
          )}
          <span class="text-xs text-gray-400 ml-2">j/k files · v toggle viewed · a approve</span>
        </div>
        <div class="flex items-center gap-2">
          <button
            onClick={() =>
              (sidebarMode.value = sidebarMode.value === "hunks" ? null : "hunks")
            }
            disabled={!diffHtml}
            aria-pressed={sidebarMode.value === "hunks"}
            title={
              !diffHtml
                ? "Select a file to enable hunk review"
                : sidebarMode.value === "hunks"
                  ? "Close hunk review sidebar"
                  : "Open hunk review sidebar"
            }
            class={`text-xs px-3 py-1.5 rounded-lg border font-medium disabled:opacity-50 disabled:cursor-not-allowed ${
              sidebarMode.value === "hunks"
                ? "border-accent text-accent bg-accent/10"
                : "border-gray-300 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700"
            }`}
          >
            🔍 Review hunks
          </button>
          {activeOrg.value && projectId && repoId && (
            <ReviewPR
              projectId={projectId}
              repoId={repoId}
              prId={prId}
              prTitle={`PR #${prId}`}
            />
          )}
          <ApprovalBar onVote={handleApprove} />
        </div>
      </div>

      {/* Body: File Tree + Diff */}
      <div class="flex flex-1 overflow-hidden">
        <aside
          class="border-r border-gray-200 dark:border-gray-800 shrink-0 relative"
          style={{ width: `${fileTreeResize.width}px` }}
        >
          <div class="h-full overflow-y-auto">
            <FileTree files={prFiles.value} onToggleViewed={handleToggleViewed} />
          </div>
          <div
            onMouseDown={fileTreeResize.onMouseDown}
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize file tree"
            title="Drag to resize"
            class="absolute top-0 right-0 bottom-0 w-1.5 -mr-0.5 cursor-col-resize hover:bg-accent/40 active:bg-accent/70 z-10"
          />
        </aside>

        <div class="flex-1 overflow-y-auto">
          {loading ? (
            <div class="flex items-center justify-center h-full text-gray-400 text-sm">Loading...</div>
          ) : diffHtml ? (
            <DiffViewer
              html={diffHtml}
              path={diffPath}
              threads={threads}
              onComment={handlePostComment}
              projectId={projectId!}
              repoId={repoId!}
              sourceCommit={sourceCommit}
              baseCommit={baseCommit}
              view={diffView.value}
            />
          ) : (
            <div class="flex items-center justify-center h-full text-gray-400 text-sm">
              Select a file to view its diff
            </div>
          )}
        </div>

        {sidebarMode.value === "hunks" && diffHtml && (
          <HunkReview
            key={diffPath}
            filePath={diffPath}
            oldContent={oldContent}
            newContent={newContent}
            reviewContext={
              activeOrg.value && projectId && repoId && sourceCommit
                ? {
                    orgUrl: activeOrg.value.orgUrl,
                    projectId,
                    repoId,
                    sourceCommit,
                  }
                : undefined
            }
            onClose={() => (sidebarMode.value = null)}
          />
        )}

        {sidebarMode.value === "pr-review" && projectId && repoId && (
          <PRReviewSidebar
            projectId={projectId}
            repoId={repoId}
            prId={prId}
            prTitle={`PR #${prId}`}
          />
        )}
      </div>
    </div>
  );
}
