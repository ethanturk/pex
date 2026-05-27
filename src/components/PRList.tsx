import { useState, useEffect, useMemo } from "preact/hooks";
import { currentView, selectedProject, selectedRepo } from "@/lib/signals";
import { listProjects, listRepositories, listPullRequests, getCurrentUserId, type Project, type Repository, type PullRequest } from "@/lib/api";

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
  all: "All statuses",
  active: "Active",
  draft: "Draft",
  completed: "Completed",
  abandoned: "Abandoned",
};

export function PRList() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [repos, setRepos] = useState<Repository[]>([]);
  const [prs, setPrs] = useState<PullRequest[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  // Filter state
  const [statusFilter, setStatusFilter] = useState("all");
  const [searchQuery, setSearchQuery] = useState("");
  const [userId, setUserId] = useState<string>("");

  useEffect(() => {
    listProjects().then(setProjects);
    getCurrentUserId().then(setUserId).catch(() => setUserId(""));
  }, []);

  // Load the repo list whenever the selected project changes (or on remount, so
  // returning from a PR detail still has the dropdown populated).
  // Do NOT clear selectedRepo here — that only belongs in the project dropdown's
  // onChange, otherwise navigating back from a PR wipes the user's selection.
  useEffect(() => {
    if (selectedProject.value) {
      listRepositories(selectedProject.value).then(setRepos);
    } else {
      setRepos([]);
    }
  }, [selectedProject.value]);

  useEffect(() => {
    if (selectedProject.value && selectedRepo.value) {
      setLoading(true);
      setError("");
      listPullRequests(selectedProject.value, selectedRepo.value)
        .then((list) => {
          setPrs(list);
          setError("");
        })
        .catch((e) => {
          setPrs([]);
          setError(typeof e === "string" ? e : e?.message ?? String(e));
        })
        .finally(() => setLoading(false));
    }
  }, [selectedRepo.value]);

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
      {/* Filters */}
      <div class="flex items-center gap-3 px-4 py-3 border-b border-gray-200 dark:border-gray-800 shrink-0">
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
          class="px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none"
        >
          <option value="">Select project...</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
        <select
          value={selectedRepo.value}
          onChange={(e) => (selectedRepo.value = e.currentTarget.value)}
          disabled={!selectedProject.value}
          class="px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none disabled:opacity-50"
        >
          <option value="">Select repository...</option>
          {repos.map((r) => (
            <option key={r.id} value={r.id}>{r.name}</option>
          ))}
        </select>

        {/* PR-level filters */}
        {prs.length > 0 && (
          <>
            <select
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.currentTarget.value)}
              class="px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none"
            >
              {Object.entries(STATUS_LABEL).map(([value, label]) => (
                <option key={value} value={value}>{label}</option>
              ))}
            </select>
            <input
              type="text"
              value={searchQuery}
              onInput={(e) => setSearchQuery(e.currentTarget.value)}
              placeholder="Search title, author, branch..."
              class="px-3 py-1.5 rounded-lg border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800 text-sm outline-none placeholder-gray-400 flex-1 min-w-0"
            />
          </>
        )}
      </div>

      {/* PR list */}
      <div class="flex-1 overflow-y-auto">
        {loading && (
          <div class="flex items-center justify-center py-12 text-gray-400 text-sm">
            Loading pull requests...
          </div>
        )}
        {!loading && error && (
          <div class="mx-4 my-3 px-3 py-2 rounded-lg border border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/20 text-sm text-red-700 dark:text-red-300 break-words">
            Failed to load pull requests: {error}
          </div>
        )}
        {!loading && !error && filteredPrs.length === 0 && selectedProject.value && selectedRepo.value && (
          <div class="flex items-center justify-center py-12 text-gray-400 text-sm">
            {prs.length === 0
              ? "No open pull requests found."
              : "No PRs match the current filters."}
          </div>
        )}
        {filteredPrs.map((pr) => (
          <button
            key={pr.pullRequestId}
            class="w-full text-left px-4 py-3 border-b border-gray-100 dark:border-gray-800 hover:bg-gray-50 dark:hover:bg-gray-800/50 flex items-start gap-3"
            onClick={() => openPR(pr.pullRequestId)}
          >
            <div class="flex-1 min-w-0">
              <div class="font-medium text-sm truncate">{pr.title}</div>
              <div class="flex items-center gap-2 mt-1 text-xs text-gray-500 dark:text-gray-400">
                <span class="font-mono">#{pr.pullRequestId}</span>
                <span>·</span>
                <span>{pr.createdBy.displayName}</span>
                <span>·</span>
                <span class="font-mono">{pr.sourceRefName.replace("refs/heads/", "")}</span>
                <span>→</span>
                <span class="font-mono">{pr.targetRefName.replace("refs/heads/", "")}</span>
              </div>
            </div>
            <div class="flex items-center gap-1.5 shrink-0">
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
