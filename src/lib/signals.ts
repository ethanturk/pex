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
export const selectedProject = signal<string>("");
export const selectedRepo = signal<string>("");

// ---- PR Review State (per-PR) ----
export interface FileEntry {
  path: string;
  status: "add" | "edit" | "delete" | "rename";
  viewed: boolean;
}
export const prFiles = signal<FileEntry[]>([]);
export const selectedFile = signal<string | null>(null);
export const currentIteration = signal<number>(1);
