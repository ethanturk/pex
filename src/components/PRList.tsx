import { useState, useEffect, useMemo, useCallback, useRef } from "preact/hooks";
import { currentView, selectedProject, selectedRepo, showPrChecks, reviewRuns, hydrateReviewRun } from "@/lib/signals";
import { listProjects, listRepositories, listPullRequests, getCurrentUserId, getPrChecks, listCompletedReviews, deleteCompletedReview, type Project, type Repository, type PullRequest, type PRCheck } from "@/lib/api";
import { getPrCheckRollup, describeChecksError } from "@/lib/prChecks";
import { considerAutoReview } from "@/lib/autoReview";

interface PRChecksState {
  loading: boolean;
  checks: PRCheck[];
  error: string;
}

const VOTE_TAG: Record<number, { label: string; class: string }> = {
  10: { label: "Approved", class: "bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300" },
  5: { label: "Approved w/ Suggestions", class: "bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300" },
  [-5]: { label: "Waiting for Author", class: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300" },
  [-10]: { label: "Rejected", class: "bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300" },
};

const STATUS_CLASS: Record<string, string> = {
  active: "status-active",
  draft: "status-draft",
  completed: "status-completed",
  abandoned: "status-abandoned",
};

const STATUS_LABEL: Record<string, string> = {
  all: "All",
  active: "Active",
  draft: "Draft",
  completed: "Completed",
  abandoned: "Abandoned",
};

function PRCheckSummary({ state }: { state: PRChecksState | undefined }) {
  if (!state || state.loading) {
    return (
      <div class="mt-2 text-xs text-gray-400 dark:text-gray-500">
        ↻ Loading checks...
      </div>
    );
  }
  if (state.error) {
    return (
      <div class="mt-2 text-xs text-red-500 dark:text-red-400" title={state.error}>
        Checks unavailable
      </div>
    );
  }
  if (state.checks.length === 0) {
    return (
      <div class="mt-2 text-xs text-gray-400 dark:text-gray-500">
        No configured build checks
      </div>
    );
  }

  const rollup = getPrCheckRollup(state.checks);
  const tone =
    rollup.status === "fail"
      ? "text-red-600 dark:text-red-400"
      : rollup.status === "running"
        ? "text-blue-600 dark:text-blue-400"
        : "text-green-600 dark:text-green-400";
  const icon = rollup.status === "fail" ? "×" : rollup.status === "running" ? "↻" : "✓";

  return (
    <div
      class={`mt-2 text-xs ${tone}`}
      title={rollup.tooltip}
    >
      <span class="inline-flex items-center gap-1">
        <span class="inline-flex items-center justify-center w-4 h-4 rounded-full border border-current text-[10px]">
          {icon}
        </span>
        <span>{rollup.requiredText}{rollup.optionalText ? ` · ${rollup.optionalText}` : ""}</span>
      </span>
    </div>
  );
}

function ReviewBadge({ prId }: { prId: number }) {
  const run = reviewRuns.value.get(prId);
  if (!run) return null;
  if (run.status === "running") {
    return (
      <span class="text-xs px-2 py-0.5 rounded-full font-medium bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-300 inline-flex items-center gap-1">
        <span class="animate-spin w-2.5 h-2.5 border border-blue-400 border-t-transparent rounded-full" />
        Reviewing
      </span>
    );
  }
  if (run.status === "error") {
    return (
      <span class="text-xs px-2 py-0.5 rounded-full font-medium bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300">
        Review failed
      </span>
    );
  }
  // A completed review (marked done, or its findings posted) is handled — show a
  // subdued badge rather than the attention-grabbing finding count.
  if (run.lifecycle === "completed") {
    return (
      <span class="text-xs px-2 py-0.5 rounded-full font-medium bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300">
        ✓ Reviewed
      </span>
    );
  }
  if (run.status === "posted") {
    return (
      <span class="text-xs px-2 py-0.5 rounded-full font-medium bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300">
        Review posted
      </span>
    );
  }
  // done / posting
  const n = run.output?.findings.length ?? 0;
  const blocking = run.output?.findings.filter((f) => f.tier === "blocking").length ?? 0;
  return (
    <span
      class={`text-xs px-2 py-0.5 rounded-full font-medium ${
        blocking > 0
          ? "bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300"
          : "bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-200"
      }`}
      title={`AI review: ${n} finding${n === 1 ? "" : "s"}${blocking > 0 ? `, ${blocking} blocking` : ""}`}
    >
      🔍 {blocking > 0 ? `${blocking} blocking` : `${n} finding${n === 1 ? "" : "s"}`}
    </span>
  );
}

// Reconcile persisted reviews against the freshly-fetched PR list for one repo:
// hydrate the badge/tab for still-active PRs, and delete stored reviews for PRs
// that have since closed/merged/abandoned. PRs absent from `list` are left
// untouched (they may simply not be loaded), so we never delete on a guess.
async function syncPersistedReviews(
  projectId: string,
  repoId: string,
  list: PullRequest[],
) {
  let stored;
  try {
    stored = await listCompletedReviews();
  } catch {
    return;
  }
  const statusById = new Map(list.map((pr) => [pr.pullRequestId, pr.status]));
  for (const review of stored) {
    if (review.projectId !== projectId || review.repoId !== repoId) continue;
    const status = statusById.get(review.prId);
    if (status === undefined) continue; // PR not in this listing — leave as-is.
    if (status === "active") {
      hydrateReviewRun(review);
    } else {
      // PR is completed/abandoned: drop the durable review and any badge.
      void deleteCompletedReview(projectId, repoId, review.prId).catch(() => {});
      if (reviewRuns.value.has(review.prId)) {
        const next = new Map(reviewRuns.value);
        next.delete(review.prId);
        reviewRuns.value = next;
      }
    }
  }
}

export function PRList() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [repos, setRepos] = useState<Repository[]>([]);
  const [prs, setPrs] = useState<PullRequest[]>([]);
  const [prChecks, setPrChecks] = useState<Record<number, PRChecksState>>({});
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const checksReqId = useRef(0);
  const checksEnabled = showPrChecks.value;

  // Filter state
  const [statusFilter, setStatusFilter] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [userId, setUserId] = useState<string>("");

  useEffect(() => {
    listProjects().then((list) => {
      setProjects(list);
      // With a single project there's nothing to choose — auto-select it so the
      // user lands straight on its repos instead of picking from a list of one.
      if (list.length === 1 && !selectedProject.value) {
        selectedProject.value = list[0].id;
      }
    });
    getCurrentUserId().then(setUserId).catch(() => setUserId(""));
  }, []);

  useEffect(() => {
    if (selectedProject.value) {
      listRepositories(selectedProject.value).then(setRepos);
    } else {
      setRepos([]);
    }
  }, [selectedProject.value]);

  const refreshPullRequests = useCallback(() => {
    if (!selectedProject.value || !selectedRepo.value) return;
    setLoading(true);
    setError("");
    const project = selectedProject.value;
    const repo = selectedRepo.value;
    listPullRequests(project, repo)
      .then((list) => {
        setPrs(list);
        setError("");
        // Restore persisted reviews so finished reviews survive restarts: the
        // PR-list badge (ReviewBadge reads reviewRuns) lights up again, and the
        // Review tab can rehydrate. Closed/merged/abandoned PRs get their stored
        // review deleted — a review is only meaningful while the PR is active.
        void syncPersistedReviews(project, repo, list);
        // Phase 4: let the auto-reviewer consider these PRs (no-op unless the
        // user enabled auto-review). Only reviewable PRs — active, non-draft.
        const reviewable = list
          .filter((pr) => pr.status === "active" && !pr.isDraft)
          .map((pr) => ({
            prId: pr.pullRequestId,
            prTitle: pr.title,
            iterationCount: pr.iterationCount,
          }));
        void considerAutoReview(project, repo, reviewable);
      })
      .catch((e) => {
        setPrs([]);
        setError(typeof e === "string" ? e : e?.message ?? String(e));
      })
      .finally(() => setLoading(false));
  }, [selectedProject.value, selectedRepo.value]);

  useEffect(() => {
    refreshPullRequests();
  }, [refreshPullRequests]);

  const loadChecksForPrs = useCallback((list: PullRequest[]) => {
    const reqId = ++checksReqId.current;
    if (!checksEnabled || !selectedProject.value || !selectedRepo.value || list.length === 0) {
      setPrChecks({});
      return;
    }

    const projectId = selectedProject.value;
    const repoId = selectedRepo.value;
    setPrChecks(
      Object.fromEntries(
        list.map((pr) => [pr.pullRequestId, { loading: true, checks: [], error: "" }]),
      ),
    );

    let nextIndex = 0;
    const workerCount = Math.min(4, list.length);
    const runWorker = async () => {
      while (nextIndex < list.length) {
        const pr = list[nextIndex++];
        try {
          const checks = await getPrChecks(projectId, repoId, pr.pullRequestId);
          if (reqId !== checksReqId.current) return;
          setPrChecks((prev) => ({
            ...prev,
            [pr.pullRequestId]: { loading: false, checks, error: "" },
          }));
        } catch (e) {
          if (reqId !== checksReqId.current) return;
          setPrChecks((prev) => ({
            ...prev,
            [pr.pullRequestId]: {
              loading: false,
              checks: [],
              error: describeChecksError(e),
            },
          }));
        }
      }
    };

    void Promise.all(Array.from({ length: workerCount }, runWorker));
  }, [checksEnabled, selectedProject.value, selectedRepo.value]);

  useEffect(() => {
    loadChecksForPrs(prs);
  }, [prs, loadChecksForPrs]);

  // Client-side filtering
  const filteredPrs = useMemo(() => {
    let result = prs;
    if (statusFilter === "draft") {
      result = result.filter((pr) => pr.isDraft);
    } else if (statusFilter === "active") {
      // Match ADO's own UI: "Active" excludes drafts.
      result = result.filter((pr) => pr.status === "active" && !pr.isDraft);
    } else if (statusFilter !== "all") {
      result = result.filter((pr) => pr.status === statusFilter);
    }
    if (searchQuery.trim()) {
      const q = searchQuery.toLowerCase();
      result = result.filter(
        (pr) =>
          pr.title.toLowerCase().includes(q) ||
          pr.createdBy.displayName.toLowerCase().includes(q) ||
          pr.sourceRefName.toLowerCase().includes(q),
      );
    }
    return result;
  }, [prs, statusFilter, searchQuery]);

  const openPR = (prId: number) => {
    currentView.value = { kind: "pr-detail", prId };
  };

  return (
    <div class="flex flex-col h-full">
      {/* Filters — responsive: stack on mobile, row on desktop */}
      <div class="flex flex-col sm:flex-row gap-2 sm:gap-3 px-3 sm:px-4 py-2 sm:py-3 border-b border-gray-200 dark:border-gray-800 shrink-0">
        {/* Project + Repo row */}
        <div class="flex gap-2 w-full sm:w-auto">
          <select
            value={selectedProject.value}
            onChange={(e) => {
              const next = e.currentTarget.value;
              if (next !== selectedProject.value) {
                selectedRepo.value = "";
                setPrs([]);
              }
              selectedProject.value = next;
            }}
            class="flex-1 sm:flex-none px-3 py-2 sm:py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none truncate min-w-0"
          >
            <option value="">Project...</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
          <select
            value={selectedRepo.value}
            onChange={(e) => (selectedRepo.value = e.currentTarget.value)}
            disabled={!selectedProject.value}
            class="flex-1 sm:flex-none px-3 py-2 sm:py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none disabled:opacity-50 truncate min-w-0"
          >
            <option value="">Repo...</option>
            {repos.map((r) => (
              <option key={r.id} value={r.id}>{r.name}</option>
            ))}
          </select>
        </div>

        {/* Status + Search + Refresh row — shown when PRs are loaded */}
        {prs.length > 0 && (
          <div class="flex gap-2 w-full sm:w-auto sm:flex-1">
            <select
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.currentTarget.value)}
              class="sm:flex-none px-3 py-2 sm:py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none"
            >
              {Object.entries(STATUS_LABEL).map(([value, label]) => (
                <option key={value} value={value}>{label}</option>
              ))}
            </select>
            <button
              onClick={refreshPullRequests}
              disabled={loading}
              title="Refresh pull requests"
              aria-label="Refresh pull requests"
              class="w-8 h-8 shrink-0 rounded-full border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-gray-500 dark:text-gray-400 hover:text-gray-800 dark:hover:text-gray-100 hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              ↻
            </button>
            <input
              type="text"
              value={searchQuery}
              onInput={(e) => setSearchQuery(e.currentTarget.value)}
              placeholder="Search title, author, branch..."
              class="flex-1 min-w-0 px-3 py-2 sm:py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none placeholder-gray-400"
            />
          </div>
        )}
      </div>

      {/* PR list */}
      <div class="flex-1 overflow-y-auto scroll-ios">
        {/* Empty states */}
        {!selectedProject.value && !selectedRepo.value && (
          <div class="flex items-center justify-center py-12 text-gray-400 text-sm px-4 text-center">
            Select a project and repository to view pull requests.
          </div>
        )}
        {selectedProject.value && selectedRepo.value && loading && (
          <div class="flex items-center justify-center py-12 text-gray-400 text-sm">
            Loading pull requests...
          </div>
        )}
        {!loading && error && (
          <div class="mx-3 sm:mx-4 my-3 px-3 py-2 rounded-lg border border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/20 text-sm text-red-700 dark:text-red-300 break-words">
            {error}
          </div>
        )}
        {!loading && !error && filteredPrs.length === 0 && selectedProject.value && selectedRepo.value && (
          <div class="flex flex-col items-center justify-center py-12 text-gray-400 text-sm gap-2">
            <span>{prs.length === 0 ? "No open pull requests found." : "No PRs match the current filters."}</span>
            <button
              onClick={refreshPullRequests}
              class="px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-xs hover:bg-gray-50 dark:hover:bg-gray-700"
            >
              Refresh
            </button>
          </div>
        )}

        {filteredPrs.map((pr) => (
          <button
            key={pr.pullRequestId}
            class="w-full text-left px-3 sm:px-4 py-3 border-b border-gray-100 dark:border-gray-800 hover:bg-gray-50 dark:hover:bg-gray-800/50 flex items-start gap-3 tappable"
            onClick={() => openPR(pr.pullRequestId)}
          >
            <div class="flex-1 min-w-0">
              <div class="font-medium text-sm truncate">{pr.title}</div>
              <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-1 text-xs text-gray-500 dark:text-gray-400">
                <span class="font-mono">#{pr.pullRequestId}</span>
                <span class="hidden sm:inline">·</span>
                <span class="truncate max-w-[120px]">{pr.createdBy.displayName}</span>
                <span class="hidden sm:inline">·</span>
                <span class="font-mono truncate max-w-[140px]">{pr.sourceRefName.replace("refs/heads/", "")}</span>
                <span class="hidden sm:inline">→</span>
                <span class="font-mono truncate max-w-[140px] hidden sm:inline">{pr.targetRefName.replace("refs/heads/", "")}</span>
              </div>
              {checksEnabled && (
                <PRCheckSummary state={prChecks[pr.pullRequestId]} />
              )}
            </div>
            <div class="flex items-center gap-1.5 shrink-0">
              <ReviewBadge prId={pr.pullRequestId} />
              {(() => {
                const mine = userId
                  ? pr.reviewers.find((r) => r.id.toLowerCase() === userId.toLowerCase())
                  : undefined;
                const tag = mine ? VOTE_TAG[mine.vote] : undefined;
                return tag ? (
                  <span class={`text-xs px-2 py-0.5 rounded-full font-medium ${tag.class}`}>
                    {tag.label}
                  </span>
                ) : null;
              })()}
              {pr.isDraft && (
                <span class={`text-xs px-2 py-0.5 rounded-full font-medium ${STATUS_CLASS.draft}`}>
                  Draft
                </span>
              )}
              <span class={`text-xs px-2 py-0.5 rounded-full font-medium ${STATUS_CLASS[pr.status] || ""}`}>
                {pr.status}
              </span>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
