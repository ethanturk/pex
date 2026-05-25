import { useState, useEffect } from "preact/hooks";
import { currentView, selectedProject, selectedRepo } from "@/lib/signals";
import { listProjects, listRepositories, listPullRequests, type Project, type Repository, type PullRequest } from "@/lib/api";

const STATUS_CLASS: Record<string, string> = {
  active: "status-active",
  draft: "status-draft",
  completed: "status-completed",
  abandoned: "status-abandoned",
};

export function PRList() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [repos, setRepos] = useState<Repository[]>([]);
  const [prs, setPrs] = useState<PullRequest[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    listProjects().then(setProjects);
  }, []);

  useEffect(() => {
    if (selectedProject.value) {
      listRepositories(selectedProject.value).then(setRepos);
      selectedRepo.value = "";
      setPrs([]);
    }
  }, [selectedProject.value]);

  useEffect(() => {
    if (selectedProject.value && selectedRepo.value) {
      setLoading(true);
      listPullRequests(selectedProject.value, selectedRepo.value)
        .then(setPrs)
        .finally(() => setLoading(false));
    }
  }, [selectedRepo.value]);

  const openPR = (prId: number) => {
    currentView.value = { kind: "pr-detail", prId };
  };

  return (
    <div class="flex flex-col h-full">
      {/* Filters */}
      <div class="flex items-center gap-3 px-4 py-3 border-b border-gray-200 dark:border-gray-800 shrink-0">
        <select
          value={selectedProject.value}
          onChange={(e) => (selectedProject.value = e.currentTarget.value)}
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
      </div>

      {/* PR list */}
      <div class="flex-1 overflow-y-auto">
        {loading && (
          <div class="flex items-center justify-center py-12 text-gray-400 text-sm">
            Loading pull requests...
          </div>
        )}
        {!loading && prs.length === 0 && selectedProject.value && selectedRepo.value && (
          <div class="flex items-center justify-center py-12 text-gray-400 text-sm">
            No open pull requests found.
          </div>
        )}
        {prs.map((pr) => (
          <button
            key={pr.pullRequestId}
            class="w-full text-left px-4 py-3 border-b border-gray-100 dark:border-gray-800 hover:bg-gray-50 dark:hover:bg-gray-800/50 flex items-start gap-3"
            onClick={() => openPR(pr.pullRequestId)}
          >
            <div class="flex-1 min-w-0">
              <div class="font-medium text-sm truncate">{pr.title}</div>
              <div class="flex items-center gap-2 mt-1 text-xs text-gray-500 dark:text-gray-400">
                <span>{pr.createdBy.displayName}</span>
                <span>·</span>
                <span class="font-mono">{pr.sourceRefName.replace("refs/heads/", "")}</span>
                <span>→</span>
                <span class="font-mono">{pr.targetRefName.replace("refs/heads/", "")}</span>
              </div>
            </div>
            <span class={`text-xs px-2 py-0.5 rounded-full font-medium shrink-0 ${STATUS_CLASS[pr.status] || ""}`}>
              {pr.status}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}
