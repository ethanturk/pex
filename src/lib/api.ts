import { invoke } from "@tauri-apps/api/core";
import type { FileEntry } from "./signals";

// ============= Types =============

export interface PullRequest {
  pullRequestId: number;
  title: string;
  description: string;
  // ADO's `status` is never "draft" by itself — drafts come back as
  // status="active" plus a separate isDraft flag.
  status: "active" | "completed" | "abandoned";
  isDraft: boolean;
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

export async function getCurrentUserId(): Promise<string> {
  return invoke<string>("get_current_user_id");
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

// ============= AI =============

export interface AiSettingsNoKey {
  provider: string;
  endpoint: string;
  model: string;
  requestTimeoutSecs: number;
  hunkConcurrency: number;
  standardsMaxChars: number;
}

export async function getAiSettings(): Promise<AiSettingsNoKey> {
  return invoke<AiSettingsNoKey>("get_ai_settings");
}

export async function saveAiSettings(
  provider: string,
  endpoint: string,
  model: string,
  apiKey: string,
  requestTimeoutSecs: number,
  hunkConcurrency: number,
  standardsMaxChars: number,
): Promise<void> {
  return invoke("save_ai_settings", {
    provider,
    endpoint,
    model,
    apiKey,
    requestTimeoutSecs,
    hunkConcurrency,
    standardsMaxChars,
  });
}

export interface ReviewHunkContext {
  orgUrl: string;
  projectId: string;
  repoId: string;
  sourceCommit: string;
}

export interface AiPromptInfo {
  key: string;
  label: string;
  description: string;
  value: string;
  defaultValue: string;
  isCustomized: boolean;
}

export async function getAiPrompts(): Promise<AiPromptInfo[]> {
  return invoke<AiPromptInfo[]>("get_ai_prompts");
}

export async function saveAiPrompt(key: string, value: string): Promise<void> {
  return invoke("save_ai_prompt", { key, value });
}

export async function resetAiPrompt(key: string): Promise<void> {
  return invoke("reset_ai_prompt", { key });
}

export async function explainHunk(
  filePath: string,
  oldContent: string,
  newContent: string,
  hunkIndex: number,
): Promise<string> {
  return invoke<string>("explain_hunk", { filePath, oldContent, newContent, hunkIndex });
}

export async function testAiConnection(): Promise<string> {
  return invoke<string>("test_ai_connection");
}

// ---- Native PR Review ----

export type Severity = "critical" | "moderate" | "minor";

export interface ReviewFinding {
  filePath: string;
  severity: Severity;
  lineStart: number | null;
  lineEnd: number | null;
  comment: string;
}

export async function postReviewFinding(
  projectId: string,
  repoId: string,
  prId: number,
  filePath: string | null,
  lineStart: number | null,
  lineEnd: number | null,
  content: string,
): Promise<CommentThread> {
  return invoke<CommentThread>("post_review_finding", {
    projectId,
    repoId,
    prId,
    filePath,
    lineStart,
    lineEnd,
    content,
  });
}

export interface ReviewOutput {
  summary: string;
  findings: ReviewFinding[];
}

export async function startReview(
  projectId: string,
  repoId: string,
  prId: number,
  prTitle: string,
): Promise<ReviewOutput> {
  return invoke<ReviewOutput>("start_review", { projectId, repoId, prId, prTitle });
}

export async function startReviewPost(
  projectId: string,
  repoId: string,
  prId: number,
  prTitle: string,
): Promise<void> {
  return invoke("start_review_post", { projectId, repoId, prId, prTitle });
}

export async function cancelReview(): Promise<void> {
  return invoke("cancel_review");
}

export async function getSavedReview(): Promise<ReviewState | null> {
  return invoke<ReviewState | null>("get_saved_review");
}

export async function clearSavedReview(): Promise<void> {
  return invoke("clear_saved_review");
}

export interface ReviewState {
  prKey: string;
  phase: string;
  filePaths: string[];
  currentFileIdx: number;
  currentFileHunks: number;
  currentHunk: number;
  currentFileFindings: [number, string][];
  completedFiles: [string, string][];
  batchSummaries: string[];
  currentBatch: number;
  totalBatches: number;
  finalReview: string | null;
}

export interface HunkLine {
  kind: string;       // "+", "-", or " "
  newLineno: number | null;
  oldLineno: number | null;
  content: string;
}

export interface DiffHunk {
  index: number;
  header: string;     // "@@ -1,4 +1,5 @@"
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: HunkLine[];
}

export async function getDiffHunks(
  oldContent: string,
  newContent: string,
): Promise<DiffHunk[]> {
  return invoke<DiffHunk[]>("get_diff_hunks", { oldContent, newContent });
}

export async function reviewHunk(
  filePath: string,
  oldContent: string,
  newContent: string,
  hunkIndex: number,
  ctx?: ReviewHunkContext,
): Promise<string> {
  return invoke<string>("review_hunk", {
    filePath,
    oldContent,
    newContent,
    hunkIndex,
    orgUrl: ctx?.orgUrl ?? null,
    projectId: ctx?.projectId ?? null,
    repoId: ctx?.repoId ?? null,
    sourceCommit: ctx?.sourceCommit ?? null,
  });
}
