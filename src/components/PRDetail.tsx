import { useState, useEffect, useCallback } from "preact/hooks";
import { currentView, prFiles, selectedFile, currentIteration } from "@/lib/signals";
import {
  getPrFiles,
  getViewedFiles,
  markFileViewed,
  getFileDiff,
  updateReviewerStatus,
  type FileDiff,
} from "@/lib/api";
import { FileTree } from "@/components/FileTree";
import { DiffViewer } from "@/components/DiffViewer";
import { ApprovalBar } from "@/components/ApprovalBar";

// These are derived from the current org/project/repo — we'd plumb them via signals
// For now, we read from localStorage or the activeOrg signal
import { activeOrg } from "@/lib/signals";

interface Props {
  prId: number;
}

export function PRDetail({ prId }: Props) {
  const [diff, setDiff] = useState<FileDiff | null>(null);
  const [loading, setLoading] = useState(false);
  const [iterationCount, setIterationCount] = useState(1);

  // Derive project/repo from state — in real impl, these come from the PR list selection
  const [projectId, setProjectId] = useState(localStorage.getItem("pex-project") || "");
  const [repoId, setRepoId] = useState(localStorage.getItem("pex-repo") || "");

  const loadFiles = useCallback(async () => {
    if (!projectId || !repoId) return;
    setLoading(true);
    try {
      const files = await getPrFiles(projectId, repoId, prId, currentIteration.value);
      const viewed = await getViewedFiles(projectId, repoId, prId);
      const viewedSet = new Set(viewed);
      prFiles.value = files.map((f) => ({ ...f, viewed: viewedSet.has(f.path) }));
    } finally {
      setLoading(false);
    }
  }, [projectId, repoId, prId]);

  const loadDiff = useCallback(async (path: string) => {
    if (!projectId || !repoId) return;
    setLoading(true);
    try {
      const d = await getFileDiff(projectId, repoId, prId, path, currentIteration.value);
      setDiff(d);
    } finally {
      setLoading(false);
    }
  }, [projectId, repoId, prId]);

  useEffect(() => {
    loadFiles();
  }, [loadFiles]);

  useEffect(() => {
    const unsub1 = selectedFile.subscribe((path) => {
      if (path) loadDiff(path);
    });
    const unsub2 = currentIteration.subscribe(() => {
      loadFiles();
      if (selectedFile.value) loadDiff(selectedFile.value);
    });
    return () => {
      unsub1();
      unsub2();
    };
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
        </div>
        <ApprovalBar onVote={handleApprove} />
      </div>

      {/* Body: File Tree + Diff */}
      <div class="flex flex-1 overflow-hidden">
        {/* File Tree Sidebar */}
        <aside class="w-64 border-r border-gray-200 dark:border-gray-800 overflow-y-auto shrink-0">
          <FileTree files={prFiles.value} onToggleViewed={handleToggleViewed} />
        </aside>

        {/* Diff / Empty State */}
        <div class="flex-1 overflow-y-auto">
          {loading ? (
            <div class="flex items-center justify-center h-full text-gray-400 text-sm">Loading...</div>
          ) : diff ? (
            <DiffViewer diff={diff} />
          ) : (
            <div class="flex items-center justify-center h-full text-gray-400 text-sm">
              Select a file to view its diff
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
