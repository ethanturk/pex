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

// Paths in the order they appear in the file tree pane, respecting current
// view mode and folder-collapse state. Used for j/k navigation so it matches
// what the user actually sees.
export const visibleFilePaths = signal<string[]>([]);
