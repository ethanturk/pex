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
  authorId: string;
  content: string;
  publishedDate: string;
  canEdit: boolean;
}

export interface VoteHistoryEntry {
  threadId: number;
  reviewerId: string;
  reviewerName: string;
  vote: number;
  publishedDate: string;
  content: string;
}

// ============= Auth =============

export async function loginPat(
  provider: "ado" | "github",
  orgUrl: string,
  pat: string,
): Promise<boolean> {
  return invoke<boolean>("login_pat", { provider, orgUrl, pat });
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

export async function getPullRequest(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<PullRequest> {
  return invoke<PullRequest>("get_pull_request", { projectId, repoId, prId });
}

export async function getPrChecks(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<PRCheck[]> {
  return invoke<PRCheck[]>("get_pr_checks", { projectId, repoId, prId });
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

export async function getVoteHistory(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<VoteHistoryEntry[]> {
  return invoke<VoteHistoryEntry[]>("get_vote_history", { projectId, repoId, prId });
}

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

export async function updateComment(
  projectId: string,
  repoId: string,
  prId: number,
  threadId: number,
  commentId: number,
  content: string,
  isPrLevel: boolean,
): Promise<Comment> {
  return invoke<Comment>("update_comment", {
    projectId,
    repoId,
    prId,
    threadId,
    commentId,
    content,
    isPrLevel,
  });
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

export interface AiProviderConfig {
  id: string;
  name: string;
  provider: string;
  endpoint: string;
  model: string;
  reasoningEffort?: string | null;
  hasApiKey: boolean;
  connectTimeoutSecs: number;
  readTimeoutSecs: number;
}

export interface AiSettingsNoKey {
  defaultProviderId: string;
  providers: AiProviderConfig[];
  provider: string;
  endpoint: string;
  model: string;
  reasoningEffort?: string | null;
  /// Whether an API key is stored for the current provider. The key itself is
  /// never returned — the UI shows a masked placeholder when this is true.
  hasApiKey: boolean;
  /// TCP/TLS handshake budget. Catches dead servers fast.
  connectTimeoutSecs: number;
  /// Stalled-stream guard: max time between successive bytes from the server.
  /// Does NOT bound total generation — a slow model that keeps the connection
  /// alive will be allowed to finish.
  readTimeoutSecs: number;
  hunkConcurrency: number;
  /// Output token cap for per-hunk passes (the main lever on local-LLM cost).
  hunkMaxTokens: number;
  /// Output token cap for the aggregation stages (adjudicate + synthesis).
  aggregateMaxTokens: number;
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
  /// Opt-in: write a JSONL diagnostic trace per review run.
  aiDiagnostics: boolean;
}

export async function getAiSettings(): Promise<AiSettingsNoKey> {
  return invoke<AiSettingsNoKey>("get_ai_settings");
}

/// Persist the "AI Defaults" (provider/endpoint/model/key/timeouts) and
/// reconfigure the live provider. An empty `apiKey` keeps the stored key.
/// Requires an explicit Save in the UI (after a successful Test).
export async function saveAiDefaults(
  provider: string,
  endpoint: string,
  model: string,
  apiKey: string,
  reasoningEffort: string | null,
  connectTimeoutSecs: number,
  readTimeoutSecs: number,
): Promise<void> {
  return invoke("save_ai_defaults", {
    provider,
    endpoint,
    model,
    apiKey,
    reasoningEffort,
    connectTimeoutSecs,
    readTimeoutSecs,
  });
}

/// Persist one provider entry. An empty `apiKey` keeps the stored key. When
/// `makeDefault` is true this provider becomes the active review provider.
export async function saveAiProviderConfig(
  providerConfig: AiProviderConfig,
  apiKey: string,
  makeDefault: boolean,
): Promise<void> {
  return invoke("save_ai_provider_config", { providerConfig, apiKey, makeDefault });
}

export async function removeAiProvider(providerId: string): Promise<void> {
  return invoke("remove_ai_provider", { providerId });
}

/// Test the AI Defaults form WITHOUT persisting. Returns the provider's model
/// list on success (used to validate the key and populate the Model dropdown).
/// An empty `apiKey` falls back to the stored key.
export async function testAiDefaults(
  provider: string,
  endpoint: string,
  apiKey: string,
  providerId?: string,
): Promise<string[]> {
  return invoke<string[]>("test_ai_defaults", { provider, endpoint, apiKey, providerId });
}

/// Persist the review/automation preferences. Autosaved on change; never
/// touches provider credentials.
export async function saveAiPreferences(prefs: {
  hunkConcurrency: number;
  hunkMaxTokens: number;
  aggregateMaxTokens: number;
  standardsMaxChars: number;
  retryCount: number;
  confidenceThreshold: number;
  blockingConfidence: number;
  autoVoteOnBlocking: boolean;
  incrementalReview: boolean;
  autoReview: boolean;
  autoPostBlocking: boolean;
  autoPostConfidence: number;
  aiDiagnostics: boolean;
}): Promise<void> {
  return invoke("save_ai_preferences", prefs);
}

/// The directory where opt-in review diagnostic traces (.jsonl) are written.
export async function getDiagnosticsDir(): Promise<string> {
  return invoke<string>("get_diagnostics_dir");
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
  /// Per-specialist buckets (Thorough mode). A finding merged from several
  /// specialists credits each, so these may sum to more than `total`.
  bySpecialist: CalibrationBucket[];
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
    sources: finding.sources ?? [],
  });
}

/// Remove a recorded verdict for a finding. Used to undo a dismissal without
/// counting the finding as accepted.
export async function clearFindingVerdict(
  projectId: string,
  repoId: string,
  prId: number,
  finding: ReviewFinding,
): Promise<void> {
  return invoke("clear_finding_verdict", {
    projectId,
    repoId,
    prId,
    filePath: finding.filePath ?? "",
    comment: finding.comment,
  });
}

export async function getReviewCalibration(): Promise<CalibrationStats> {
  return invoke<CalibrationStats>("get_review_calibration");
}

export async function clearReviewFeedback(): Promise<void> {
  return invoke("clear_review_feedback");
}

export interface AiPromptInfo {
  key: string;
  label: string;
  description: string;
  value: string;
  defaultValue: string;
  isCustomized: boolean;
  /// Per-prompt provider override. null/empty = use the AI tab's default provider.
  providerId: string | null;
  /// Per-prompt model override. null/empty = use the AI tab's default provider/model.
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

/// Persist a per-prompt provider/model override. Pass an empty model to clear.
export async function saveAiPromptModel(
  key: string,
  model: string,
  providerId?: string,
): Promise<void> {
  return invoke("save_ai_prompt_model", { key, model, providerId });
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

export async function listAiProviderModels(providerId: string): Promise<string[]> {
  return invoke<string[]>("list_ai_provider_models", { providerId });
}

export async function explainHunk(
  filePath: string,
  oldContent: string,
  newContent: string,
  hunkIndex: number,
): Promise<string> {
  return invoke<string>("explain_hunk", { filePath, oldContent, newContent, hunkIndex });
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
  /// Specialist label(s) that raised this finding; drives per-specialist
  /// calibration. Empty when unattributed (e.g. Fast mode).
  sources: string[];
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
  health?: "success" | "degraded" | "failed";
  warnings?: number;
  providerFailures?: number;
}

export type ReviewSkipReason =
  | "deleted"
  | "binary"
  | "excluded"
  | "notIncluded"
  | "unsupportedPath"
  | "diffUnavailable"
  | "noChanges"
  | string;

export interface ReviewRuleMatch {
  source: string;
  pattern: string | null;
  title: string;
  rule: string;
}

export interface ReviewPreviewFile {
  path: string;
  status: string;
  willReview: boolean;
  skipReason: ReviewSkipReason | null;
  hunkCount: number;
  changedLines: number;
  ruleSource: string | null;
  rulePattern: string | null;
  ruleTitle: string | null;
  relatedFiles: string[];
}

export interface ReviewPreview {
  iteration: number;
  totalFiles: number;
  reviewableFiles: number;
  skippedFiles: number;
  totalHunks: number;
  changedLines: number;
  files: ReviewPreviewFile[];
}

/// Review strategy. "fast" = single generalist pass per hunk (original).
/// "thorough" = multi-pass with specialist agents (slower, broader coverage).
export type ReviewMode = "fast" | "thorough";

export async function previewReview(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<ReviewPreview> {
  return invoke<ReviewPreview>("preview_review", { projectId, repoId, prId });
}

export async function startReview(
  projectId: string,
  repoId: string,
  prId: number,
  prTitle: string,
  mode: ReviewMode = "fast",
  enabledSpecialists?: string[],
  resume = false,
): Promise<ReviewOutput> {
  return invoke<ReviewOutput>("start_review", {
    projectId,
    repoId,
    prId,
    prTitle,
    mode,
    enabledSpecialists: enabledSpecialists ?? null,
    resume,
  });
}

/// One Thorough-mode specialist plus the concrete model it will use. Shown in
/// the pre-review confirmation dialog so the user can pick which agents run.
export interface ReviewSpecialistInfo {
  key: string;
  label: string;
  description: string;
  model: string;
  providerName: string;
  /** Selected by default in the confirm dialog (the lean default roster). */
  defaultEnabled: boolean;
}

export async function getReviewSpecialists(): Promise<ReviewSpecialistInfo[]> {
  return invoke<ReviewSpecialistInfo[]>("get_review_specialists");
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

/// Lifecycle of a persisted completed review.
/// - `outstanding`: finished, not yet handled (shows a PR-list badge).
/// - `completed`: marked done by the user, or its findings were posted to ADO.
export type ReviewLifecycle = "outstanding" | "completed";

/// A completed review persisted in SQLite, surviving restarts until completed.
export interface StoredReview {
  prKey: string;
  projectId: string;
  repoId: string;
  prId: number;
  prTitle: string;
  iteration: number;
  status: ReviewLifecycle;
  output: ReviewOutput;
  updatedAt: string;
}

export async function getCompletedReview(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<StoredReview | null> {
  return invoke<StoredReview | null>("get_completed_review", { projectId, repoId, prId });
}

export async function listCompletedReviews(): Promise<StoredReview[]> {
  return invoke<StoredReview[]>("list_completed_reviews");
}

export async function completeReview(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<void> {
  return invoke("complete_review", { projectId, repoId, prId });
}

export async function deleteCompletedReview(
  projectId: string,
  repoId: string,
  prId: number,
): Promise<void> {
  return invoke("delete_completed_review", { projectId, repoId, prId });
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

// ============= Cross-device sync =============

/**
 * Status of the optional cross-device sync layer. `lastSync` is an RFC3339
 * timestamp; `framesSynced` is the count reconciled on the last sync.
 */
export interface SyncStatus {
  enabled: boolean;
  url: string;
  configured: boolean;
  syncing: boolean;
  lastSync: string | null;
  lastError: string | null;
  framesSynced: number | null;
}

export async function getSyncStatus(): Promise<SyncStatus> {
  return invoke<SyncStatus>("get_sync_status");
}

/**
 * Enable sync against a remote libsql/Turso DB. Pass an empty `token` to keep
 * the previously stored token (the UI never echoes the real token back).
 */
export async function enableSync(url: string, token: string): Promise<SyncStatus> {
  return invoke<SyncStatus>("enable_sync", { url, token });
}

export async function disableSync(): Promise<SyncStatus> {
  return invoke<SyncStatus>("disable_sync");
}

export async function syncNow(): Promise<SyncStatus> {
  return invoke<SyncStatus>("sync_now");
}
