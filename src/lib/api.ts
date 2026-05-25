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
  oldContent: string;
  newContent: string;
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
): Promise<FileDiff> {
  return invoke<FileDiff>("get_file_diff", { projectId, repoId, prId, filePath, iteration });
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
  line: number,
  content: string,
): Promise<CommentThread> {
  return invoke<CommentThread>("post_comment", {
    projectId,
    repoId,
    prId,
    filePath,
    line,
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

// ============= AI =============

export interface AiSettingsNoKey {
  provider: string;
  endpoint: string;
  model: string;
}

export interface PuristCheckResult {
  ok: boolean;
  message: string;
}

export async function getAiSettings(): Promise<AiSettingsNoKey> {
  return invoke<AiSettingsNoKey>("get_ai_settings");
}

export async function saveAiSettings(
  provider: string,
  endpoint: string,
  model: string,
  apiKey: string,
): Promise<void> {
  return invoke("save_ai_settings", { provider, endpoint, model, apiKey });
}

export async function explainDiff(
  filePath: string,
  oldContent: string,
  newContent: string,
): Promise<string> {
  return invoke<string>("explain_diff", { filePath, oldContent, newContent });
}

export async function checkPurist(puristPath: string): Promise<PuristCheckResult> {
  return invoke<PuristCheckResult>("check_purist", { puristPath });
}

export async function getPuristPath(): Promise<string | null> {
  return invoke<string | null>("get_purist_path");
}

export async function savePuristPath(path: string): Promise<void> {
  return invoke("save_purist_path", { path });
}

export async function testAiConnection(): Promise<string> {
  return invoke<string>("test_ai_connection");
}

export async function reviewPrDryRun(
  orgUrl: string,
  project: string,
  repo: string,
  prId: number,
): Promise<void> {
  return invoke("review_pr_dry_run", { orgUrl, project, repo, prId });
}

export async function reviewPrPost(
  orgUrl: string,
  project: string,
  repo: string,
  prId: number,
): Promise<void> {
  return invoke("review_pr_post", { orgUrl, project, repo, prId });
}

export async function cancelReview(): Promise<void> {
  return invoke("cancel_review");
}
