import { signal } from "@preact/signals";

// ---- Theme ----
export type Theme = "system" | "light" | "dark";
export const theme = signal<Theme>(
  (localStorage.getItem("pex-theme") as Theme) || "system",
);

export function applyTheme(t: Theme) {
  const root = document.documentElement;
  if (t === "dark" || (t === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches)) {
    root.classList.add("dark");
  } else {
    root.classList.remove("dark");
  }
  localStorage.setItem("pex-theme", t);
}

// Init on load
applyTheme(theme.value);
theme.subscribe(applyTheme);

// ---- Diff view (inline vs side-by-side) ----
export type DiffView = "inline" | "split";
export const diffView = signal<DiffView>(
  (localStorage.getItem("pex-diff-view") as DiffView) || "inline",
);
diffView.subscribe((v) => localStorage.setItem("pex-diff-view", v));

// ---- File tree view (flat list vs nested folders) ----
export type FileTreeMode = "flat" | "tree";
export const fileTreeMode = signal<FileTreeMode>(
  (localStorage.getItem("pex-file-tree-mode") as FileTreeMode) || "flat",
);
fileTreeMode.subscribe((v) => localStorage.setItem("pex-file-tree-mode", v));

// ---- PR list settings ----
export const showPrChecks = signal<boolean>(
  localStorage.getItem("pex-show-pr-checks") === "true",
);
showPrChecks.subscribe((v) => localStorage.setItem("pex-show-pr-checks", String(v)));

// ---- Auth ----
export interface OrgCredential {
  orgUrl: string;
  name: string;
  tokenType: "pat" | "oauth";
}
export const activeOrg = signal<OrgCredential | null>(null);
export const savedOrgs = signal<OrgCredential[]>([]);

// ---- Navigation ----
export type View =
  | { kind: "auth" }
  | { kind: "org-select" }
  | { kind: "pr-list" }
  | { kind: "pr-detail"; prId: number };
export const currentView = signal<View>({ kind: "auth" });

// ---- PR Selection Context (project/repo carried from PRList → PRDetail) ----
// Persisted per-org in localStorage so the project/repo from the last session is
// restored on app launch once the org is reactivated.
export const selectedProject = signal<string>("");
export const selectedRepo = signal<string>("");

const projectKey = (orgUrl: string) => `pex-last-project:${orgUrl}`;
const repoKey = (orgUrl: string) => `pex-last-repo:${orgUrl}`;

// Hydrate on org activation, then persist on subsequent edits. Switching orgs
// loads that org's last selection (or "" if none was saved).
activeOrg.subscribe((org) => {
  if (!org) return;
  selectedProject.value = localStorage.getItem(projectKey(org.orgUrl)) ?? "";
  selectedRepo.value = localStorage.getItem(repoKey(org.orgUrl)) ?? "";
});

selectedProject.subscribe((v) => {
  const org = activeOrg.value;
  if (!org) return;
  if (v) localStorage.setItem(projectKey(org.orgUrl), v);
  else localStorage.removeItem(projectKey(org.orgUrl));
});

selectedRepo.subscribe((v) => {
  const org = activeOrg.value;
  if (!org) return;
  if (v) localStorage.setItem(repoKey(org.orgUrl), v);
  else localStorage.removeItem(repoKey(org.orgUrl));
});

// ---- PR Review State (per-PR) ----
export interface FileEntry {
  path: string;
  status: "add" | "edit" | "delete" | "rename";
  viewed: boolean;
}
export const prFiles = signal<FileEntry[]>([]);
export const selectedFile = signal<string | null>(null);
export const currentIteration = signal<number>(1);

// Set by anything that wants to jump the DiffViewer to a specific 1-based new-side
// line (e.g. clicking a review finding). DiffViewer consumes it once and resets to null.
export const pendingScrollLine = signal<number | null>(null);

// Paths in the order they appear in the file tree pane, respecting current
// view mode and folder-collapse state. Used for j/k navigation so it matches
// what the user actually sees.
export const visibleFilePaths = signal<string[]>([]);

// Bumped whenever a PR comment thread is created outside the DiffViewer flow
// (e.g. posting a single finding from the PR review sidebar). PRDetail watches
// this to refetch threads so the Comments pane stays in sync with ADO.
export const threadsRefreshTick = signal<number>(0);

// ---- PR Review (background, per-PR) ----
// The Rust engine runs a review serially (one resumable state at a time), so we
// track a single "active" PR — but each PR keeps its last result so a user can
// navigate away mid-review and come back to it, or revisit a finished review.
export interface ReviewProgress {
  phase: string;
  detail: string;
  fileNum?: number;
  totalFiles?: number;
  hunk?: number;
  totalHunks?: number;
  batch?: number;
  totalBatches?: number;
  fileCount?: number;
}

export type PRReviewStatus = "running" | "done" | "posting" | "posted" | "error";

export type ReviewMode = "fast" | "thorough";

export interface PRReviewRun {
  projectId: string;
  repoId: string;
  prTitle: string;
  status: PRReviewStatus;
  /// Review mode the user picked when starting this run.
  mode?: ReviewMode;
  progress: ReviewProgress | null;
  // Output of the latest completed run; preserved across "posting" so the
  // sidebar can keep showing the summary while we post to ADO.
  output: {
    summary: string;
    findings: {
      filePath: string;
      severity: "critical" | "moderate" | "minor";
      lineStart: number | null;
      lineEnd: number | null;
      comment: string;
    }[];
  } | null;
  error: string | null;
}

export const reviewRuns = signal<Map<number, PRReviewRun>>(new Map());

// PR currently being processed by the Rust engine. Used to route global
// `review-progress` / `review-done` events to the right run, and to disable
// "Review PR" on other PRs while one is in flight.
export const activeReviewPrId = signal<number | null>(null);

// Which right-side sidebar is open in the PR detail view. Hunk review and PR
// review share the slot.
export type SidebarMode = "hunks" | "pr-review" | null;
export const sidebarMode = signal<SidebarMode>(null);

export function updateReviewRun(prId: number, patch: Partial<PRReviewRun> | ((prev: PRReviewRun | undefined) => PRReviewRun)) {
  const next = new Map(reviewRuns.value);
  const prev = next.get(prId);
  if (typeof patch === "function") {
    next.set(prId, patch(prev));
  } else if (prev) {
    next.set(prId, { ...prev, ...patch });
  }
  reviewRuns.value = next;
}
