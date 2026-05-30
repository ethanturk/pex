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

export interface PRCheck {
  id: string;
  name: string;
  status: "queued" | "running" | "approved" | "rejected" | "notApplicable" | "broken" | string;
  isRequired: boolean;
  description: string;
  startedDate: string | null;
  completedDate: string | null;
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

export async function getPrChecks(
  projectId: string,
  prId: number,
): Promise<PRCheck[]> {
  return invoke<PRCheck[]>("get_pr_checks", { projectId, prId });
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

export interface PrefetchDiffsResult {
  cached: number;
  fetched: number;
  failed: number;
}

export async function prefetchPrDiffs(
  projectId: string,
  repoId: string,
  prId: number,
  iteration: number,
  filePaths: string[],
): Promise<PrefetchDiffsResult> {
  return invoke<PrefetchDiffsResult>("prefetch_pr_diffs", {
    projectId,
    repoId,
    prId,
    iteration,
    filePaths,
  });
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
  /// TCP/TLS handshake budget. Catches dead servers fast.
  connectTimeoutSecs: number;
  /// Stalled-stream guard: max time between successive bytes from the server.
  /// Does NOT bound total generation — a slow model that keeps the connection
  /// alive will be allowed to finish.
  readTimeoutSecs: number;
  hunkConcurrency: number;
  standardsMaxChars: number;
  /// Number of retries after a failed LLM call in a PR review.
  /// 0 = no retries (recommended for slow local providers).
  retryCount: number;
  /// Minimum confidence (0–100) a finding must reach to be reported.
  /// 0 surfaces everything; higher values raise the precision bar.
  confidenceThreshold: number;
  /// Confidence (0–100) at/above which a Critical finding is tiered Blocking
  /// (the "critical line"). Below it, criticals are Should-fix.
  blockingConfidence: number;
  /// Opt-in: cast a "wait for author" vote when posting a review that has at
  /// least one blocking finding.
  autoVoteOnBlocking: boolean;
  /// Opt-in: review only files changed since the last reviewed iteration.
  incrementalReview: boolean;
  /// Opt-in: auto-trigger a review on a new PR / iteration.
  autoReview: boolean;
  /// Opt-in: after an auto-review, auto-post high-confidence blocking findings.
  autoPostBlocking: boolean;
  /// Confidence floor (0–100) for auto-posting a blocking finding.
  autoPostConfidence: number;
}

export async function getAiSettings(): Promise<AiSettingsNoKey> {
  return invoke<AiSettingsNoKey>("get_ai_settings");
}

export async function saveAiSettings(
  provider: string,
  endpoint: string,
  model: string,
  apiKey: string,
  connectTimeoutSecs: number,
  readTimeoutSecs: number,
  hunkConcurrency: number,
  standardsMaxChars: number,
  retryCount: number,
  confidenceThreshold: number,
  blockingConfidence: number,
  autoVoteOnBlocking: boolean,
  incrementalReview: boolean,
  autoReview: boolean,
  autoPostBlocking: boolean,
  autoPostConfidence: number,
): Promise<void> {
  return invoke("save_ai_settings", {
    provider,
    endpoint,
    model,
    apiKey,
    connectTimeoutSecs,
    readTimeoutSecs,
    hunkConcurrency,
    standardsMaxChars,
    retryCount,
    confidenceThreshold,
    blockingConfidence,
    autoVoteOnBlocking,
    incrementalReview,
    autoReview,
    autoPostBlocking,
    autoPostConfidence,
  });
}

// ---- Phase 4: automation ----

/// Of the given PRs (with their current iteration counts), return the IDs that
/// should be auto-reviewed. Returns [] when auto-review is disabled.
export async function autoReviewCandidates(
  projectId: string,
  repoId: string,
  prs: { prId: number; iterationCount: number }[],
): Promise<number[]> {
  return invoke<number[]>("auto_review_candidates", { projectId, repoId, prs });
}

/// Auto-post the high-confidence blocking findings from a completed review.
/// Returns the number posted (0 when auto-post is disabled).
export async function autoPostReviewFindings(
  projectId: string,
  repoId: string,
  prId: number,
  findings: ReviewFinding[],
): Promise<number> {
  return invoke<number>("auto_post_review_findings", { projectId, repoId, prId, findings });
}

// ---- Review feedback loop (Phase 3) ----

export type Verdict = "accepted" | "dismissed" | "edited";

export interface CalibrationBucket {
  label: string;
  accepted: number;
  dismissed: number;
  edited: number;
  acceptRate: number | null;
}

export interface CalibrationStats {
  total: number;
  accepted: number;
  dismissed: number;
  edited: number;
  acceptRate: number | null;
  bySeverity: CalibrationBucket[];
  byTier: CalibrationBucket[];
}

/// Record a reviewer's verdict on a finding. Dismissed findings are suppressed
/// on future review runs for this PR.
export async function recordFindingVerdict(
  projectId: string,
  repoId: string,
  prId: number,
  verdict: Verdict,
  finding: ReviewFinding,
): Promise<void> {
  return invoke("record_finding_verdict", {
    projectId,
    repoId,
    prId,
    verdict,
    filePath: finding.filePath ?? "",
    comment: finding.comment,
    severity: finding.severity,
    tier: finding.tier,
    confidence: finding.confidence,
  });
}

export async function getReviewCalibration(): Promise<CalibrationStats> {
  return invoke<CalibrationStats>("get_review_calibration");
}

export async function clearReviewFeedback(): Promise<void> {
  return invoke("clear_review_feedback");
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
  /// Per-prompt model override. null/empty = use the AI tab's default model.
  model: string | null;
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

/// Persist a per-prompt model override. Pass an empty string to clear.
export async function saveAiPromptModel(key: string, model: string): Promise<void> {
  return invoke("save_ai_prompt_model", { key, model });
}

export async function resetAiPromptModel(key: string): Promise<void> {
  return invoke("reset_ai_prompt_model", { key });
}

/// Returns the list of model IDs available from the configured AI provider.
/// When `refresh` is false, the cached list is returned if available;
/// otherwise the provider's /models endpoint is hit.
export async function listAiModels(refresh = false): Promise<string[]> {
  return invoke<string[]>("list_ai_models", { refresh });
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

/// Triage tier derived from severity + confidence + anchor. Blocking and
/// should-fix are "pulled forward"; nit and fyi are "pushed back".
export type Tier = "blocking" | "should-fix" | "nit" | "fyi";

export interface ReviewFinding {
  filePath: string;
  severity: Severity;
  /// How sure the reviewer is the finding is real (0–100), distinct from
  /// severity (how bad it is if real).
  confidence: number;
  /// Triage tier (blocking → fyi).
  tier: Tier;
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

/// Review strategy. "fast" = single generalist pass per hunk (original).
/// "thorough" = multi-pass with specialist agents (slower, broader coverage).
export type ReviewMode = "fast" | "thorough";

export async function startReview(
  projectId: string,
  repoId: string,
  prId: number,
  prTitle: string,
  mode: ReviewMode = "fast",
): Promise<ReviewOutput> {
  return invoke<ReviewOutput>("start_review", { projectId, repoId, prId, prTitle, mode });
}

export async function startReviewPost(
  projectId: string,
  repoId: string,
  prId: number,
  prTitle: string,
  mode: ReviewMode = "fast",
): Promise<void> {
  return invoke("start_review_post", { projectId, repoId, prId, prTitle, mode });
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
  mode: ReviewMode;
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
