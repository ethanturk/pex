import { invoke } from "@tauri-apps/api/core";
import type { FileEntry } from "./signals";

// ============= Types =============

export interface PullRequest {
  pullRequestId: number;
  title: string;
  description: string;
  status: "active" | "completed" | "abandoned" | "draft";
  createdBy: { displayName: string; id: string };
  sourceRefName: string;
  targetRefName: string;
  creationDate: string;
  mergeStatus: string;
  reviewers: Reviewer[];
  iterationCount: number;
}

export interface Reviewer {
  id: string;
  displayName: string;
  vote: number;
  isRequired: boolean;
}

export interface FileDiff {
  html: string;
  path: string;
  status: string;
  sourceCommit: string;
  baseCommit: string | null;
}

export interface CommentThread {
  id: number;
  filePath: string;
  lineStart: number;
  lineEnd: number;
  status: string;
  comments: Comment[];
}

export interface Comment {
  id: number;
  author: string;
  content: string;
  publishedDate: string;
}

// ============= Auth =============

export async function loginPat(orgUrl: string, pat: string): Promise<boolean> {
  return invoke<boolean>("login_pat", { orgUrl, pat });
}

export async function loginOAuth(
  orgUrl: string,
  clientId: string,
  clientSecret: string,
): Promise<{ access_token: string; expires_in: number }> {
  return invoke<{ access_token: string; expires_in: number }>("login_oauth", {
    orgUrl,
    clientId,
    clientSecret,
  });
}

export async function refreshOAuthToken(
  orgUrl: string,
): Promise<{ access_token: string; expires_in: number }> {
  return invoke<{ access_token: string; expires_in: number }>("refresh_oauth_token", {
    orgUrl,
  });
}

export async function getSavedOrgs(): Promise<any[]> {
  return invoke<any[]>("get_saved_orgs");
}

export async function removeOrg(orgUrl: string): Promise<void> {
  return invoke("remove_org", { orgUrl });
}

export async function activateOrg(orgUrl: string): Promise<boolean> {
  return invoke<boolean>("activate_org", { orgUrl });
}

// ============= Projects & Repos =============

export interface Project {
  id: string;
  name: string;
}

export interface Repository {
  id: string;
  name: string;
  defaultBranch: string;
}

export async function listProjects(): Promise<Project[]> {
  return invoke<Project[]>("list_projects");
}

export async function listRepositories(projectId: string): Promise<Repository[]> {
  return invoke<Repository[]>("list_repositories", { projectId });
}

// ============= PRs =============

export async function listPullRequests(
  projectId: string,
  repoId: string,
): Promise<PullRequest[]> {
  return invoke<PullRequest[]>("list_pull_requests", { projectId, repoId });
}

// ============= Iterations =============

export interface Iteration {
  id: number;
  name: string;
}

export async function getIterations(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<Iteration[]> {
  return invoke<Iteration[]>("get_iterations", { projectId, repoId, prId });
}

// ============= Files & Diffs =============

export async function getPrFiles(
  projectId: string,
  repoId: string,
  prId: number,
  iteration: number,
): Promise<FileEntry[]> {
  return invoke<FileEntry[]>("get_pr_files", { projectId, repoId, prId, iteration });
}

export async function getFileDiff(
  projectId: string,
  repoId: string,
  prId: number,
  filePath: string,
  iteration: number,
  view: "inline" | "split" = "inline",
): Promise<FileDiff> {
  return invoke<FileDiff>("get_file_diff", { projectId, repoId, prId, filePath, iteration, view });
}

export async function getFileLines(
  projectId: string,
  repoId: string,
  commitId: string,
  filePath: string,
  startLine: number,
  endLine: number,
): Promise<string[]> {
  return invoke<string[]>("get_file_lines", {
    projectId,
    repoId,
    commitId,
    filePath,
    startLine,
    endLine,
  });
}

export async function markFileViewed(
  projectId: string,
  repoId: string,
  prId: number,
  filePath: string,
  viewed: boolean,
): Promise<void> {
  return invoke("mark_file_viewed", { projectId, repoId, prId, filePath, viewed });
}

export async function getViewedFiles(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<string[]> {
  return invoke<string[]>("get_viewed_files", { projectId, repoId, prId });
}

// ============= Comments =============

export async function getThreads(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<CommentThread[]> {
  return invoke<CommentThread[]>("get_threads", { projectId, repoId, prId });
}

export async function postComment(
  projectId: string,
  repoId: string,
  prId: number,
  filePath: string,
  lineStart: number,
  lineEnd: number,
  content: string,
): Promise<CommentThread> {
  return invoke<CommentThread>("post_comment", {
    projectId,
    repoId,
    prId,
    filePath,
    lineStart,
    lineEnd,
    content,
  });
}

export async function postReply(
  projectId: string,
  repoId: string,
  prId: number,
  threadId: number,
  content: string,
): Promise<Comment> {
  return invoke<Comment>("post_reply", { projectId, repoId, prId, threadId, content });
}

// ============= PR Actions =============

export async function updateReviewerStatus(
  projectId: string,
  repoId: string,
  prId: number,
  vote: number,
): Promise<void> {
  return invoke("update_reviewer_status", { projectId, repoId, prId, vote });
}
