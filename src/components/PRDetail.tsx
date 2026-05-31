import { useState, useEffect, useCallback, useRef } from "preact/hooks";
import { useResizableWidth } from "@/lib/useResizableWidth";
import { currentView, prFiles, selectedFile, currentIteration, selectedProject, selectedRepo, activeOrg, diffView, visibleFilePaths, sidebarMode, threadsRefreshTick, showPrChecks, openTabs, previewPath, activeTab, PR_REVIEW_TAB, resetTabs } from "@/lib/signals";
import {
  getPrChecks,
  getPullRequest,
  getPrFiles,
  getViewedFiles,
  markFileViewed,
  getFileDiff,
  prefetchPrDiffs,
  updateReviewerStatus,
  getThreads,
  postComment,
  getIterations,
  type CommentThread,
  type PullRequest,
  type PRCheck,
  type FileDiff,
} from "@/lib/api";
import { getPrCheckRollup, describeChecksError } from "@/lib/prChecks";
import { FileTree } from "@/components/FileTree";
import { DiffViewer } from "@/components/DiffViewer";
import { HunkReview } from "@/components/HunkReview";
import { PRReviewPanel } from "@/components/PRReviewPanel";
import { TabBar } from "@/components/TabBar";
import { ApprovalBar } from "@/components/ApprovalBar";

interface Props { prId: number; }

function branchName(refName: string): string {
  return refName.replace(/^refs\/heads\//, "");
}

interface PRChecksState {
  loading: boolean;
  checks: PRCheck[];
  error: string;
}

function PRDetailCheckIcon({
  state,
  onRefresh,
}: {
  state: PRChecksState;
  onRefresh: () => void;
}) {
  if (state.loading) {
    return (
      <button
        type="button"
        onClick={onRefresh}
        class="inline-flex items-center justify-center w-6 h-6 rounded text-base text-blue-600 dark:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800"
        title="Loading build checks."
        aria-label="Loading build checks"
      >
        <span class="pr-check-spin-slow">↻</span>
      </button>
    );
  }
  if (state.error) {
    return (
      <button
        type="button"
        onClick={onRefresh}
        class="inline-flex items-center justify-center w-6 h-6 rounded text-base font-semibold text-gray-400 dark:text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-800"
        title={`Build checks unavailable: ${state.error}`}
        aria-label="Build checks unavailable"
      >
        !
      </button>
    );
  }

  const rollup = getPrCheckRollup(state.checks);
  if (rollup.status === "none") return null;

  const className =
    rollup.status === "fail"
      ? "text-red-600 dark:text-red-400"
      : rollup.status === "running"
        ? "text-blue-600 dark:text-blue-400"
        : "text-green-600 dark:text-green-400";
  const icon = rollup.status === "fail" ? "×" : rollup.status === "running" ? "↻" : "✓";
  const sizeClass = rollup.status === "running" ? "text-base pr-check-spin-slow" : "text-base font-semibold";

  return (
    <button
      type="button"
      onClick={onRefresh}
      class={`inline-flex items-center justify-center w-6 h-6 rounded hover:bg-gray-100 dark:hover:bg-gray-800 ${sizeClass} ${className}`}
      title={`${rollup.tooltip}\nClick to refresh build checks.`}
      aria-label={`${rollup.tooltip} Click to refresh build checks.`}
    >
      {icon}
    </button>
  );
}

export function PRDetail({ prId }: Props) {
  const [diffHtml, setDiffHtml] = useState<string>("");
  const [diffPath, setDiffPath] = useState<string>("");
  const [diffStatus, setDiffStatus] = useState<string>("");
  const [sourceCommit, setSourceCommit] = useState<string>("");
  const [baseCommit, setBaseCommit] = useState<string | null>(null);
  const [oldContent, setOldContent] = useState<string>("");
  const [newContent, setNewContent] = useState<string>("");
  const [pullRequest, setPullRequest] = useState<PullRequest | null>(null);
  const [loading, setLoading] = useState(false);
  const [iterationCount, setIterationCount] = useState(1);
  const [threads, setThreads] = useState<CommentThread[]>([]);
  const [copied, setCopied] = useState(false);
  const [voteError, setVoteError] = useState<string>("");
  const [checksState, setChecksState] = useState<PRChecksState>({
    loading: false,
    checks: [],
    error: "",
  });
  const fileTreeResize = useResizableWidth({
    storageKey: "pex-filetree-width",
    defaultWidth: 256,
    min: 160,
    max: 600,
    side: "right",
  });

  const projectId = selectedProject.value;
  const repoId = selectedRepo.value;
  const checksEnabled = showPrChecks.value;
  const sourceBranch = pullRequest ? branchName(pullRequest.sourceRefName) : "";
  const targetBranch = pullRequest ? branchName(pullRequest.targetRefName) : "";

  const loadChecks = useCallback(async () => {
    if (!checksEnabled || !projectId || !repoId) {
      setChecksState({ loading: false, checks: [], error: "" });
      return;
    }
    setChecksState({ loading: true, checks: [], error: "" });
    try {
      const checks = await getPrChecks(projectId, repoId, prId);
      setChecksState({ loading: false, checks, error: "" });
    } catch (e) {
      setChecksState({
        loading: false,
        checks: [],
        error: typeof e === "string" ? e : e instanceof Error ? e.message : String(e),
      });
    }
  }, [checksEnabled, projectId, repoId, prId]);

  // Clear cross-PR state on PR switch so a stale `selectedFile` from the
  // previous PR doesn't kick off a diff load against this PR's commits —
  // which would otherwise resolve to identical content and surface the
  // "old/new identical" error before the user picks a real file.
  useEffect(() => {
    selectedFile.value = null;
    resetTabs();
    diffCache.current.clear();
    currentIteration.value = 1;
    prFiles.value = [];
    setPullRequest(null);
    setDiffHtml("");
    setDiffPath("");
    setDiffStatus("");
    setSourceCommit("");
    setBaseCommit(null);
    setOldContent("");
    setNewContent("");
    setThreads([]);
    setChecksState({ loading: false, checks: [], error: "" });
  }, [prId]);

  useEffect(() => {
    let cancelled = false;
    if (!projectId || !repoId) {
      setPullRequest(null);
      return;
    }

    getPullRequest(projectId, repoId, prId)
      .then((pr) => {
        if (!cancelled) setPullRequest(pr);
      })
      .catch((e) => {
        if (!cancelled) {
          setPullRequest(null);
          console.error("Failed to load PR details:", e);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [projectId, repoId, prId]);

  useEffect(() => {
    let cancelled = false;
    if (!checksEnabled || !projectId || !repoId) {
      setChecksState({ loading: false, checks: [], error: "" });
    } else {
      setChecksState({ loading: true, checks: [], error: "" });
      getPrChecks(projectId, repoId, prId)
        .then((checks) => {
          if (!cancelled) setChecksState({ loading: false, checks, error: "" });
        })
        .catch((e) => {
          if (!cancelled) {
            setChecksState({
              loading: false,
              checks: [],
              error: describeChecksError(e),
            });
          }
        });
    }
    return () => {
      cancelled = true;
    };
  }, [checksEnabled, projectId, repoId, prId]);

  // Fetch iterations on mount. Default to the LATEST iteration so the file
  // tree shows the full cumulative changeset — ADO's iterations/{N}/changes
  // endpoint returns only the files changed through iteration N, so picking
  // iteration 1 silently hides files added in later pushes.
  useEffect(() => {
    if (projectId && repoId) {
      getIterations(projectId, repoId, prId)
        .then((iters) => {
          if (iters.length > 0) {
            setIterationCount(iters.length);
            if (currentIteration.value !== iters.length) {
              currentIteration.value = iters.length;
            }
          }
        })
        .catch(() => {}); // Silently fall back to 1
    }
  }, [projectId, repoId, prId]);

  // Concurrent fetches can race when iteration resolves async on mount:
  // an in-flight iter=1 call can overwrite a fresher iter=N response.
  // Increment on each call and ignore stale completions.
  const filesReqId = useRef(0);
  const diffReqId = useRef(0);
  const prefetchKey = useRef("");
  // Front-end diff cache so re-activating an open tab (or j/k revisits) skips
  // the IPC refetch + DOM rebuild. Keyed by path|iteration|view since the diff
  // HTML depends on both. Cleared on PR switch.
  const diffCache = useRef<Map<string, FileDiff>>(new Map());
  const diffCacheKey = (path: string) => `${path}|${currentIteration.value}|${diffView.value}`;

  const loadFiles = useCallback(async () => {
    if (!projectId || !repoId) return;
    const reqId = ++filesReqId.current;
    setLoading(true);
    try {
      const files = await getPrFiles(projectId, repoId, prId, currentIteration.value);
      if (reqId !== filesReqId.current) return;
      const viewed = await getViewedFiles(projectId, repoId, prId);
      if (reqId !== filesReqId.current) return;
      const viewedSet = new Set(viewed);
      const filesWithViewed = files.map((f) => ({ ...f, viewed: viewedSet.has(f.path) }));
      prFiles.value = filesWithViewed;

      const paths = filesWithViewed.map((f) => f.path);
      const key = `${projectId}|${repoId}|${prId}|${currentIteration.value}|${paths.join("\n")}`;
      if (paths.length > 0 && prefetchKey.current !== key) {
        prefetchKey.current = key;
        prefetchPrDiffs(projectId, repoId, prId, currentIteration.value, paths)
          .catch((e) => console.debug("Background diff prefetch failed:", e));
      }
    } catch (e) {
      if (reqId !== filesReqId.current) return;
      console.error("Failed to load PR files:", e);
      prFiles.value = [];
    } finally {
      if (reqId === filesReqId.current) setLoading(false);
    }
  }, [projectId, repoId, prId]);

  // Warm the front-end diff cache for a file without touching displayed state,
  // so the next j/k step (or tab activation) renders instantly.
  const prefetchDiff = useCallback(async (path: string) => {
    if (!projectId || !repoId || !path) return;
    const key = diffCacheKey(path);
    if (diffCache.current.has(key)) return;
    try {
      const d = await getFileDiff(projectId, repoId, prId, path, currentIteration.value, diffView.value);
      diffCache.current.set(key, d);
    } catch {
      // Prefetch is best-effort; a real load will surface any error.
    }
  }, [projectId, repoId, prId]);

  const applyDiff = (d: FileDiff) => {
    setDiffHtml(d.html);
    setDiffPath(d.path);
    setDiffStatus(d.status);
    setSourceCommit(d.sourceCommit);
    setBaseCommit(d.baseCommit);
    setOldContent(d.oldContent);
    setNewContent(d.newContent);
  };

  const loadDiff = useCallback(async (path: string) => {
    if (!projectId || !repoId) return;
    const reqId = ++diffReqId.current;

    // Cache hit: render the diff immediately and refresh threads in the
    // background (don't block the diff on the threads round-trip).
    const cached = diffCache.current.get(diffCacheKey(path));
    if (cached) {
      applyDiff(cached);
      setLoading(false);
      getThreads(projectId, repoId, prId)
        .then((all) => {
          if (reqId === diffReqId.current) setThreads(all.filter((t: any) => t.filePath === cached.path));
        })
        .catch((e) => console.error("Failed to load threads:", e));
      return;
    }

    setLoading(true);
    try {
      const d = await getFileDiff(projectId, repoId, prId, path, currentIteration.value, diffView.value);
      if (reqId !== diffReqId.current) return;
      diffCache.current.set(diffCacheKey(path), d);
      applyDiff(d);
      // Load threads for this file
      const allThreads = await getThreads(projectId, repoId, prId);
      if (reqId !== diffReqId.current) return;
      setThreads(allThreads.filter((t: any) => t.filePath === d.path));
    } catch (e: any) {
      if (reqId !== diffReqId.current) return;
      const msg = typeof e === "string" ? e : e?.message ?? String(e);
      console.error("Failed to load file diff:", e);
      setDiffPath(path);
      setDiffStatus("");
      setDiffHtml(
        `<div class="p-4 text-sm text-red-600 dark:text-red-400 break-words">Failed to load diff: ${msg
          .replace(/&/g, "&amp;")
          .replace(/</g, "&lt;")
          .replace(/>/g, "&gt;")}</div>`
      );
      setThreads([]);
    } finally {
      if (reqId === diffReqId.current) setLoading(false);
    }
  }, [projectId, repoId, prId]);

  useEffect(() => { loadFiles(); }, [loadFiles]);

  // Reconcile `selectedFile` into the tab model: any caller that sets
  // `selectedFile` (file tree click, j/k nav, jump-to-finding from the review
  // panel) opens the file as a preview tab (if not already open) and focuses it.
  useEffect(() => {
    const unsub = selectedFile.subscribe((path) => {
      if (!path) return;
      if (!openTabs.value.includes(path) && previewPath.value !== path) {
        previewPath.value = path;
      }
      if (activeTab.value !== path) activeTab.value = path;
    });
    return unsub;
  }, []);

  useEffect(() => {
    const unsub1 = selectedFile.subscribe((path) => {
      if (!path) return;
      loadDiff(path);
      // Warm the neighbors in the order the user actually sees them so the
      // next/prev j/k step renders from cache instantly.
      const order = visibleFilePaths.value;
      const idx = order.indexOf(path);
      if (idx >= 0) {
        if (idx + 1 < order.length) prefetchDiff(order[idx + 1]);
        if (idx - 1 >= 0) prefetchDiff(order[idx - 1]);
      }
    });
    const unsub2 = currentIteration.subscribe(() => {
      loadFiles();
      if (selectedFile.value) loadDiff(selectedFile.value);
    });
    const unsub3 = diffView.subscribe(() => {
      if (selectedFile.value) loadDiff(selectedFile.value);
    });
    // When a thread is posted from outside the diff view (e.g. the PR review
    // sidebar's "Create comment"), refetch threads for the current file so the
    // Comments pane reflects what's actually in ADO.
    let firstTick = true;
    const unsub4 = threadsRefreshTick.subscribe(() => {
      if (firstTick) { firstTick = false; return; }
      if (!projectId || !repoId || !diffPath) return;
      getThreads(projectId, repoId, prId)
        .then((all) => setThreads(all.filter((t: any) => t.filePath === diffPath)))
        .catch((e) => console.error("Failed to refresh threads:", e));
    });
    return () => { unsub1(); unsub2(); unsub3(); unsub4(); };
  }, [loadDiff, loadFiles, prefetchDiff, projectId, repoId, prId, diffPath]);

  const handleToggleViewed = async (path: string, viewed: boolean) => {
    if (!projectId || !repoId) return;
    await markFileViewed(projectId, repoId, prId, path, viewed);
    prFiles.value = prFiles.value.map((f) => (f.path === path ? { ...f, viewed } : f));
  };

  const handleApprove = async (vote: number) => {
    setVoteError("");
    try {
      await updateReviewerStatus(projectId, repoId, prId, vote);
      currentView.value = { kind: "pr-list" };
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      // GitHub forbids reviewing a PR you authored; surface a plain-language
      // reason instead of the raw 422 body. (alert() is a no-op in the Tauri
      // webview, so render the error inline instead.)
      const msg = /can ?not approve your own|review your own/i.test(raw)
        ? "GitHub doesn't let you review your own pull request. Ask another collaborator to review it."
        : raw;
      console.error("Vote failed:", raw);
      setVoteError(msg);
    }
  };

  const handlePostComment = async (
    filePath: string,
    lineStart: number,
    lineEnd: number,
    content: string,
  ) => {
    await postComment(
      projectId,
      repoId,
      prId,
      filePath,
      lineStart,
      lineEnd,
      content,
    );
    // Refetch to get the canonical thread (the POST response from ADO sometimes
    // omits comment content/author fields).
    const allThreads = await getThreads(projectId, repoId, prId);
    setThreads(allThreads.filter((t: any) => t.filePath === filePath));
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
      <div class="flex items-center justify-between gap-3 px-4 py-2 border-b border-gray-200 dark:border-gray-800 shrink-0">
        <div class="flex items-center gap-3 min-w-0">
          <button
            class="text-sm text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
            onClick={() => (currentView.value = { kind: "pr-list" })}
          >
            ← Back
          </button>
          <span class="font-semibold text-sm shrink-0">PR #{prId}</span>
          {pullRequest && (
            <div
              class="hidden md:flex items-center gap-1.5 min-w-0 text-xs text-gray-500 dark:text-gray-400"
              title={`${sourceBranch} -> ${targetBranch}`}
            >
              <span class="font-mono truncate max-w-[20rem]">{sourceBranch}</span>
              <span class="shrink-0">→</span>
              <span class="font-mono truncate max-w-[20rem]">{targetBranch}</span>
            </div>
          )}
          <button
            class="text-xs px-1.5 py-0.5 rounded text-gray-400 hover:text-gray-700 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-800"
            title={copied ? "Copied!" : "Copy PR URL"}
            aria-label="Copy PR URL"
            onClick={() => {
              const org = activeOrg.value?.orgUrl?.replace(/\/$/, "");
              if (!org) return;
              const url = `${org}/${projectId}/_git/${repoId}/pullrequest/${prId}`;
              navigator.clipboard.writeText(url).then(() => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              });
            }}
          >
            {copied ? "✓" : "📋"}
          </button>
          {checksEnabled && <PRDetailCheckIcon state={checksState} onRefresh={loadChecks} />}
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
          <span class="text-xs text-gray-400 ml-2 hidden xl:inline">j/k files · v toggle viewed · a approve</span>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <button
            onClick={() =>
              (sidebarMode.value = sidebarMode.value === "hunks" ? null : "hunks")
            }
            disabled={!diffHtml}
            aria-pressed={sidebarMode.value === "hunks"}
            title={
              !diffHtml
                ? "Select a file to explain its changes"
                : sidebarMode.value === "hunks"
                  ? "Close explain sidebar"
                  : "Explain the changes in this file hunk by hunk"
            }
            class={`text-xs px-3 py-1 rounded border font-medium disabled:opacity-50 disabled:cursor-not-allowed ${
              sidebarMode.value === "hunks"
                ? "border-accent text-accent bg-accent/10"
                : "border-gray-300 dark:border-gray-600 hover:bg-gray-100 dark:hover:bg-gray-700"
            }`}
          >
            ✨ Explain
          </button>
          <ApprovalBar onVote={handleApprove} />
        </div>
      </div>
      {voteError && (
        <div class="px-4 py-2 bg-red-50 dark:bg-red-900/20 border-b border-red-200 dark:border-red-800 text-sm text-red-700 dark:text-red-300 flex items-start gap-2">
          <span class="flex-1 break-words">{voteError}</span>
          <button
            onClick={() => setVoteError("")}
            class="shrink-0 text-red-400 hover:text-red-600 dark:hover:text-red-200"
            aria-label="Dismiss"
          >
            ✕
          </button>
        </div>
      )}

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

        <div class="flex-1 overflow-hidden min-w-0 flex flex-col">
          <TabBar prId={prId} />
          <div class="flex-1 overflow-hidden min-w-0">
            {activeTab.value === PR_REVIEW_TAB ? (
              projectId && repoId ? (
                <PRReviewPanel
                  projectId={projectId}
                  repoId={repoId}
                  prId={prId}
                  prTitle={pullRequest?.title ?? `PR #${prId}`}
                />
              ) : (
                <div class="flex items-center justify-center h-full text-gray-400 text-sm">
                  Select a repository to review this PR
                </div>
              )
            ) : loading ? (
              <div class="flex items-center justify-center h-full text-gray-400 text-sm">Loading...</div>
            ) : diffHtml ? (
              <DiffViewer
                html={diffHtml}
                path={diffPath}
                status={diffStatus}
                threads={threads}
                onComment={handlePostComment}
                projectId={projectId!}
                repoId={repoId!}
                sourceCommit={sourceCommit}
                baseCommit={baseCommit}
                view={diffView.value}
                oldContent={oldContent}
                newContent={newContent}
              />
            ) : (
              <div class="flex items-center justify-center h-full text-gray-400 text-sm">
                Select a file to view its diff
              </div>
            )}
          </div>
        </div>

        {sidebarMode.value === "hunks" && diffHtml && activeTab.value !== PR_REVIEW_TAB && (
          <HunkReview
            key={diffPath}
            filePath={diffPath}
            oldContent={oldContent}
            newContent={newContent}
            onClose={() => (sidebarMode.value = null)}
          />
        )}
      </div>
    </div>
  );
}
