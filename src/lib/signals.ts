import { signal } from "@preact/signals";
import type { SyncStatus, StoredReview } from "@/lib/api";

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

// ---- Appearance: font + text sizes ----
// Discrete text-size steps shared by the app UI and the diff viewer (each
// tracked independently). Values map to concrete pixel sizes when applied.
export type TextSize = "small" | "medium" | "large" | "xl";

// App font family. The value is a CSS font-family stack; "" means "use the
// app default" (clears the inline override so the stylesheet default applies).
export const appFont = signal<string>(localStorage.getItem("pex-app-font") || "");
function applyAppFont(f: string) {
  // Inline style on <html> wins over the stylesheet default; clearing it
  // (empty string) falls back to the default sans stack. Monospace elements
  // (e.g. the diff) set their own font and are unaffected.
  document.documentElement.style.fontFamily = f;
  localStorage.setItem("pex-app-font", f);
}
applyAppFont(appFont.value);
appFont.subscribe(applyAppFont);

// App UI text size — scales the root font-size, so all rem-based sizing tracks
// it. "medium" (16px) is the browser default, i.e. today's baseline.
const APP_TEXT_PX: Record<TextSize, string> = {
  small: "14px",
  medium: "16px",
  large: "18px",
  xl: "20px",
};
export const appTextSize = signal<TextSize>(
  (localStorage.getItem("pex-app-text-size") as TextSize) || "medium",
);
function applyAppTextSize(s: TextSize) {
  document.documentElement.style.fontSize = APP_TEXT_PX[s] ?? APP_TEXT_PX.medium;
  localStorage.setItem("pex-app-text-size", s);
}
applyAppTextSize(appTextSize.value);
appTextSize.subscribe(applyAppTextSize);

// Diff viewer text size — independent of the app size. Drives CSS variables
// (see global.css `html[data-diff-size]`) so only the diff scales.
export const diffTextSize = signal<TextSize>(
  (localStorage.getItem("pex-diff-text-size") as TextSize) || "medium",
);
function applyDiffTextSize(s: TextSize) {
  document.documentElement.dataset.diffSize = s;
  localStorage.setItem("pex-diff-text-size", s);
}
applyDiffTextSize(diffTextSize.value);
diffTextSize.subscribe(applyDiffTextSize);

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
  provider: "ado" | "github";
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

// ---- Main-area tabs (VS Code style) ----
// The center area is a tab strip. Summary and PR Review are permanent, pinned
// pseudo-tabs identified by sentinels; they never appear in `openTabs`. All
// other tabs are file paths.
export const PR_SUMMARY_TAB = "__pr-summary__";
export const PR_REVIEW_TAB = "__pr-review__";
export type ActiveTab = string; // a file path, or one of the PR_*_TAB sentinels

// Pinned file tabs, in display order (never contains PR_*_TAB sentinels).
export const openTabs = signal<string[]>([]);
// The transient "preview" tab (italic, replaced by the next single-click), or
// null. A preview path is never also in `openTabs`.
export const previewPath = signal<string | null>(null);
// Which tab is focused: a file path (pinned or preview) or a PR_*_TAB sentinel.
export const activeTab = signal<ActiveTab>(PR_SUMMARY_TAB);

export function isPrMetaTab(tab: ActiveTab | null): boolean {
  return tab === PR_SUMMARY_TAB || tab === PR_REVIEW_TAB;
}

// MRU stack of focused tabs (most-recent last). Lets closeTab return to the
// previously-active tab (VS Code style) instead of a positional neighbour.
// Subscribing here captures every activation regardless of which call site set
// `activeTab` (tab click, file-tree nav, preview/pin, meta-tab focus).
const tabHistory: ActiveTab[] = [];
activeTab.subscribe((tab) => {
  const i = tabHistory.indexOf(tab);
  if (i !== -1) tabHistory.splice(i, 1);
  tabHistory.push(tab);
});

// A tab still "exists" if it's a permanent meta tab, a pinned tab, or the
// current preview. Reflects live signal state, so call after mutating them.
function tabExists(tab: ActiveTab): boolean {
  return isPrMetaTab(tab) || openTabs.value.includes(tab) || previewPath.value === tab;
}

// Single-click a file: open it as the preview tab (replacing any prior preview,
// unless it's already pinned) and focus it.
export function openPreviewTab(path: string) {
  if (!openTabs.value.includes(path)) previewPath.value = path;
  activeTab.value = path;
  selectedFile.value = path;
}

// Double-click a file (or its preview tab): pin it permanently.
export function pinTab(path: string) {
  if (!openTabs.value.includes(path)) openTabs.value = [...openTabs.value, path];
  if (previewPath.value === path) previewPath.value = null;
  activeTab.value = path;
  selectedFile.value = path;
}

export function closeTab(path: string) {
  const remaining = openTabs.value.filter((p) => p !== path);
  if (remaining.length !== openTabs.value.length) openTabs.value = remaining;
  if (previewPath.value === path) previewPath.value = null;
  // Drop the closed tab from the MRU history so it can't be re-focused.
  const hi = tabHistory.indexOf(path);
  if (hi !== -1) tabHistory.splice(hi, 1);
  if (activeTab.value === path) {
    // Return to the most-recently-active tab that still exists; fall back to
    // the last pinned tab, then the preview, then Summary.
    let next: ActiveTab | undefined;
    for (let i = tabHistory.length - 1; i >= 0; i--) {
      if (tabHistory[i] !== path && tabExists(tabHistory[i])) {
        next = tabHistory[i];
        break;
      }
    }
    next ??= remaining[remaining.length - 1] ?? previewPath.value ?? PR_SUMMARY_TAB;
    activeTab.value = next;
    selectedFile.value = isPrMetaTab(next) ? null : next;
  }
}

export function focusPrSummaryTab() {
  activeTab.value = PR_SUMMARY_TAB;
}

export function focusPrReviewTab() {
  activeTab.value = PR_REVIEW_TAB;
}

export function resetTabs() {
  openTabs.value = [];
  previewPath.value = null;
  tabHistory.length = 0;
  activeTab.value = PR_SUMMARY_TAB;
}

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
  // `plan` event: the full ordered worklist + how many were already done (resume).
  files?: string[];
  completedCount?: number;
  // `file-done` event: which file finished, and how long it took.
  fileIndex?: number;
  durationMs?: number;
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
  // Live per-file review tracking, accumulated from progress events (these
  // persist across events, unlike `progress`, which is replaced each time).
  /// Ordered list of files being reviewed (from the `plan` event).
  fileList?: string[];
  /// Completed-file durations in ms, keyed by file index.
  fileDurations?: Record<number, number>;
  /// Files already finished before this session started (resumed runs).
  preCompletedCount?: number;
  /// Index of the file currently under review (for the live timer + spinner).
  activeFileIndex?: number;
  /// Epoch ms when the active file started — drives the running timer.
  activeFileStartMs?: number;
  // Output of the latest completed run; preserved across "posting" so the
  // sidebar can keep showing the summary while we post to ADO.
  output: {
    summary: string;
    findings: {
      filePath: string;
      severity: "critical" | "moderate" | "minor";
      confidence: number;
      tier: "blocking" | "should-fix" | "nit" | "fyi";
      sources: string[];
      lineStart: number | null;
      lineEnd: number | null;
      comment: string;
    }[];
  } | null;
  error: string | null;
  // Set when this run was hydrated from a persisted (durable) review rather than
  // produced live this session. `undefined` for live runs. Drives the PR-list
  // "outstanding" badge and the "Mark completed" control.
  lifecycle?: "outstanding" | "completed";
}

export const reviewRuns = signal<Map<number, PRReviewRun>>(new Map());

// PR currently being processed by the Rust engine. Used to route global
// `review-progress` / `review-done` events to the right run, and to disable
// "Review PR" on other PRs while one is in flight.
export const activeReviewPrId = signal<number | null>(null);

// Which right-side sidebar is open in the PR detail view. Currently just the
// Explain ("hunks") panel — PR review now lives in a main-area tab.
export type SidebarMode = "hunks" | null;
export const sidebarMode = signal<SidebarMode>(null);

// Bumped when the main Explain button should explain every hunk in the current
// file. HunkReview consumes the latest value after it opens and loads hunks.
export const explainAllHunksRequest = signal<{ id: number; filePath: string | null }>({
  id: 0,
  filePath: null,
});

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

// Seed `reviewRuns` from a persisted review so a finished review reappears after
// a restart (PR-list badge + restored Review tab). Never clobbers an existing
// run — a live/in-flight review for the same PR always wins.
export function hydrateReviewRun(stored: StoredReview) {
  if (reviewRuns.value.has(stored.prId)) return;
  const next = new Map(reviewRuns.value);
  next.set(stored.prId, {
    projectId: stored.projectId,
    repoId: stored.repoId,
    prTitle: stored.prTitle,
    status: stored.status === "completed" ? "posted" : "done",
    progress: null,
    output: stored.output,
    error: null,
    lifecycle: stored.status,
  });
  reviewRuns.value = next;
}

// ---- Cross-device sync ----
// Latest known sync status, shared between the desktop settings dialog and the
// mobile Settings tab. `null` until first loaded from the backend.
export const syncStatus = signal<SyncStatus | null>(null);
