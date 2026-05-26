use crate::AppError;
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::de::DeserializeOwned;

/// Azure DevOps REST API client.
#[derive(Clone)]
pub struct AdoClient {
    pub org_url: String,
    auth_value: String, // "Basic base64" or "Bearer token"
    http: reqwest::Client,
    api_version: String,
}

impl AdoClient {
    pub fn new(org_url: String, pat: String) -> Self {
        let auth = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!(":{}", pat))
        );
        Self {
            org_url: org_url.trim_end_matches('/').to_string(),
            auth_value: auth,
            http: reqwest::Client::new(),
            api_version: "7.1".to_string(),
        }
    }

    pub fn with_bearer_token(org_url: String, token: String) -> Self {
        Self {
            org_url: org_url.trim_end_matches('/').to_string(),
            auth_value: format!("Bearer {}", token),
            http: reqwest::Client::new(),
            api_version: "7.1".to_string(),
        }
    }

    fn auth_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&self.auth_value).unwrap(),
        );
        headers
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, AppError> {
        let url = format!("{}/{}", self.org_url, path);
        let resp = self
            .http
            .get(&url)
            .headers(self.auth_headers())
            .send()
            .await?;

        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| AppError::Ado(e.to_string()))?;

        if !status.is_success() {
            return Err(AppError::Ado(format!("ADO API {}: {}", status, body)));
        }

        serde_json::from_str(&body).map_err(|e| AppError::Ado(format!("Parse error: {}", e)))
    }

    async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, AppError> {
        let url = format!("{}/{}", self.org_url, path);
        let resp = self
            .http
            .post(&url)
            .headers(self.auth_headers())
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AppError::Ado(e.to_string()))?;

        if !status.is_success() {
            return Err(AppError::Ado(format!("ADO API {}: {}", status, text)));
        }

        serde_json::from_str(&text).map_err(|e| AppError::Ado(format!("Parse error: {}", e)))
    }

    async fn patch<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, AppError> {
        let url = format!("{}/{}", self.org_url, path);
        let resp = self
            .http
            .patch(&url)
            .headers(self.auth_headers())
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AppError::Ado(e.to_string()))?;

        if !status.is_success() {
            return Err(AppError::Ado(format!("ADO API {}: {}", status, text)));
        }

        serde_json::from_str(&text).map_err(|e| AppError::Ado(format!("Parse error: {}", e)))
    }

    // ---- Projects & Repos ----

    pub async fn list_projects(&self) -> Result<Vec<ProjectSummary>, AppError> {
        #[derive(serde::Deserialize)]
        struct Response {
            value: Vec<ProjectSummary>,
        }
        let resp: Response = self
            .get(&format!("_apis/projects?api-version={}", self.api_version))
            .await?;
        Ok(resp.value)
    }

    pub async fn list_repositories(&self, project: &str) -> Result<Vec<RepoSummary>, AppError> {
        #[derive(serde::Deserialize)]
        struct Response {
            value: Vec<RepoSummary>,
        }
        let resp: Response = self
            .get(&format!(
                "{}/_apis/git/repositories?api-version={}",
                project, self.api_version
            ))
            .await?;
        Ok(resp.value)
    }

    // ---- Pull Requests ----

    pub async fn list_pull_requests(
        &self,
        project: &str,
        repo_id: &str,
    ) -> Result<Vec<PullRequest>, AppError> {
        #[derive(serde::Deserialize)]
        struct Response {
            value: Vec<PullRequest>,
        }

        let resp: Response = self
            .get(&format!(
            "{}/_apis/git/repositories/{}/pullrequests?searchCriteria.status=active&api-version={}",
            project, repo_id, self.api_version
        ))
            .await?;
        Ok(resp.value)
    }

    pub async fn get_pull_request(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
    ) -> Result<PullRequest, AppError> {
        self.get(&format!(
            "{}/_apis/git/repositories/{}/pullrequests/{}?api-version={}",
            project, repo_id, pr_id, self.api_version
        ))
        .await
    }

    pub async fn get_iterations(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
    ) -> Result<Vec<Iteration>, AppError> {
        #[derive(serde::Deserialize)]
        struct Response {
            value: Vec<Iteration>,
        }
        let resp: Response = self
            .get(&format!(
                "{}/_apis/git/repositories/{}/pullRequests/{}/iterations?api-version={}",
                project, repo_id, pr_id, self.api_version
            ))
            .await?;
        Ok(resp.value)
    }

    // ---- Files & Diffs ----

    /// Fetch a single iteration with its ref-commit pointers.
    /// `commonRefCommit` is the merge base (PR diff target); `sourceRefCommit` is the iteration HEAD.
    async fn get_iteration_detail(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        iteration_id: i32,
    ) -> Result<(String, Option<String>), AppError> {
        #[derive(serde::Deserialize)]
        struct CommitRef {
            #[serde(rename = "commitId")]
            commit_id: String,
        }
        #[derive(serde::Deserialize)]
        struct IterationDetail {
            #[serde(rename = "sourceRefCommit")]
            source_ref_commit: CommitRef,
            #[serde(rename = "commonRefCommit", default)]
            common_ref_commit: Option<CommitRef>,
        }

        let detail: IterationDetail = self
            .get(&format!(
                "{}/_apis/git/repositories/{}/pullRequests/{}/iterations/{}?api-version={}",
                project, repo_id, pr_id, iteration_id, self.api_version
            ))
            .await?;

        Ok((
            detail.source_ref_commit.commit_id,
            detail.common_ref_commit.map(|c| c.commit_id),
        ))
    }

    pub async fn get_pr_files(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        iteration: i32,
    ) -> Result<PrFilesResult, AppError> {
        // ADO returns the cumulative changes for an iteration via the iterations/{id}/changes
        // endpoint. The response uses `changeEntries`, not `changes`.
        #[derive(serde::Deserialize)]
        struct ChangesResponse {
            #[serde(rename = "changeEntries", default)]
            change_entries: Vec<FileChange>,
        }

        let changes: ChangesResponse = self
            .get(&format!(
                "{}/_apis/git/repositories/{}/pullRequests/{}/iterations/{}/changes?$top=1000&api-version={}",
                project, repo_id, pr_id, iteration, self.api_version
            ))
            .await?;

        // Filter to blob (file) entries — iteration changes can include tree entries for renames.
        let files: Vec<FileChange> = changes
            .change_entries
            .into_iter()
            .filter(|f| {
                f.item
                    .git_object_type
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case("blob"))
                    .unwrap_or(true)
            })
            .collect();

        let (commit_id, parent_commit_id) = self
            .get_iteration_detail(project, repo_id, pr_id, iteration)
            .await
            .unwrap_or_else(|_| (String::new(), None));

        Ok(PrFilesResult {
            files,
            commit_id,
            parent_commit_id,
        })
    }

    pub async fn get_file_diff(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        file_path: &str,
        iteration: i32,
        view: crate::diff::engine::DiffView,
    ) -> Result<DiffResult, AppError> {
        // Diff the iteration HEAD (sourceRefCommit) against the PR merge base (commonRefCommit),
        // not the latest source-branch commit's parent (which would give per-commit diffs).
        let (source_commit, base_commit) = self
            .get_iteration_detail(project, repo_id, pr_id, iteration)
            .await?;

        let new_snap = self
            .get_file_content(project, repo_id, file_path, &source_commit)
            .await?;

        let old_snap = match &base_commit {
            Some(bid) => self
                .get_file_content(project, repo_id, file_path, bid)
                .await?,
            None => FileSnapshot::Missing,
        };

        // Pull out the bytes (or empty string for missing/inconclusive sides).
        let old_content = match &old_snap {
            FileSnapshot::Present(s) => s.clone(),
            _ => String::new(),
        };
        let new_content = match &new_snap {
            FileSnapshot::Present(s) => s.clone(),
            _ => String::new(),
        };

        // Compute diff + syntax highlight
        let mut html =
            crate::diff::engine::highlighted_diff_view(&old_content, &new_content, file_path, view);

        // Only show the "couldn't fetch" diagnostic when we actually couldn't
        // tell what's at one or both sides. An empty `__init__.py` (Present(""))
        // is a real, valid file and should render a clean empty diff.
        let inconclusive_side = matches!(new_snap, FileSnapshot::Inconclusive)
            || matches!(old_snap, FileSnapshot::Inconclusive);
        // Two non-empty bodies that happen to match across base and source is
        // strong evidence of a fetch quirk (ADO returning identical metadata
        // for both versions); still flag those.
        let identical_nonempty = matches!(&old_snap, FileSnapshot::Present(s) if !s.is_empty())
            && matches!(&new_snap, FileSnapshot::Present(s) if !s.is_empty())
            && old_content == new_content;
        if inconclusive_side || identical_nonempty {
            let base = base_commit.as_deref().unwrap_or("<none>");
            let reason = if inconclusive_side {
                "Couldn't determine file contents from Azure DevOps — the items endpoint didn't return body bytes for one or both sides."
            } else {
                "Old and new file contents are identical — likely a content-fetch issue (e.g. ADO returned metadata instead of bytes for this file)."
            };
            html = format!(
                r#"<div class="p-4 text-sm text-amber-700 dark:text-amber-300 break-words font-mono">
                  {}
                  <br/>Path: <code>{}</code>
                  <br/>Base commit: <code>{}</code>
                  <br/>Source commit: <code>{}</code>
                </div>"#,
                reason,
                escape_html_inline(file_path),
                escape_html_inline(base),
                escape_html_inline(&source_commit),
            );
        }

        let change_type = match (&old_snap, &new_snap) {
            (FileSnapshot::Missing, _) => "add",
            (_, FileSnapshot::Missing) => "delete",
            _ => "edit",
        };

        Ok(DiffResult {
            html,
            path: file_path.to_string(),
            status: change_type.to_string(),
            source_commit,
            base_commit,
            old_content,
            new_content,
        })
    }

    /// Fetch a 1-based inclusive line range from a file at a given commit.
    /// Used by the diff context-expander UI to reveal more lines around a hunk.
    pub async fn get_file_lines(
        &self,
        project: &str,
        repo_id: &str,
        commit_id: &str,
        file_path: &str,
        start_line: usize,
        end_line: usize,
    ) -> Result<Vec<String>, AppError> {
        if start_line == 0 || end_line < start_line {
            return Ok(Vec::new());
        }
        let content = match self
            .get_file_content(project, repo_id, file_path, commit_id)
            .await?
        {
            FileSnapshot::Present(s) => s,
            FileSnapshot::Missing | FileSnapshot::Inconclusive => return Ok(Vec::new()),
        };
        let lines: Vec<String> = content
            .lines()
            .skip(start_line - 1)
            .take(end_line - start_line + 1)
            .map(|s| s.to_string())
            .collect();
        Ok(lines)
    }

    /// Fetch raw file content at a specific commit version.
    /// Returns Ok("") only when the file legitimately doesn't exist at that version
    /// (new file → not in base; deleted file → not in HEAD). Other failures bubble up.
    async fn get_file_content(
        &self,
        project: &str,
        repo_id: &str,
        file_path: &str,
        commit_id: &str,
    ) -> Result<FileSnapshot, AppError> {
        // ADO's items endpoint requires the path rooted at "/". The iteration
        // changes API is inconsistent about whether returned paths include the
        // leading slash, so normalize both cases. We percent-encode each segment
        // but leave the slashes intact — some ADO routing layers misbehave with
        // %2F-encoded path separators in the `path` query value.
        let rooted = if file_path.starts_with('/') {
            file_path.to_string()
        } else {
            format!("/{}", file_path)
        };
        let path_for_url = encode_path_preserve_slash(&rooted);

        // Try raw-text first ($format=text + download=true). When ADO honors it the
        // body is the file bytes directly. If we get JSON anyway (some flavors of
        // ADO ignore $format with certain Accept headers), parse the envelope.
        let text_url = format!(
            "{}/{}/_apis/git/repositories/{}/items?path={}&versionDescriptor.version={}&versionDescriptor.versionType=commit&download=true&$format=text&api-version={}",
            self.org_url, project, repo_id, path_for_url, commit_id, self.api_version
        );

        match self.fetch_item_body(&text_url, file_path, commit_id).await? {
            FetchOutcome::Text(s) => return Ok(FileSnapshot::Present(s)),
            FetchOutcome::Empty200 => return Ok(FileSnapshot::Present(String::new())),
            FetchOutcome::Missing => return Ok(FileSnapshot::Missing),
            FetchOutcome::Inconclusive => {} // fall through to JSON+includeContent
        }

        // Fallback: JSON envelope with includeContent=true, then read `.content`.
        let json_url = format!(
            "{}/{}/_apis/git/repositories/{}/items?path={}&versionDescriptor.version={}&versionDescriptor.versionType=commit&includeContent=true&api-version={}",
            self.org_url, project, repo_id, path_for_url, commit_id, self.api_version
        );
        match self.fetch_item_body(&json_url, file_path, commit_id).await? {
            FetchOutcome::Text(s) => Ok(FileSnapshot::Present(s)),
            FetchOutcome::Empty200 => Ok(FileSnapshot::Present(String::new())),
            FetchOutcome::Missing => Ok(FileSnapshot::Missing),
            // Both strategies couldn't decide — leave it as Inconclusive so
            // the diagnostic only fires when we truly have no answer.
            FetchOutcome::Inconclusive => Ok(FileSnapshot::Inconclusive),
        }
    }

    async fn fetch_item_body(
        &self,
        url: &str,
        file_path: &str,
        commit_id: &str,
    ) -> Result<FetchOutcome, AppError> {
        let mut headers = self.auth_headers();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("*/*"),
        );

        let resp = self
            .http
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| AppError::Ado(format!("Fetch file content failed: {}", e)))?;

        let debug = std::env::var("PEX_DEBUG_HTTP").is_ok();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if debug {
            eprintln!("[pex] GET {url} → {} ({})", resp.status(), content_type);
        }

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(FetchOutcome::Missing);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Ado(format!(
                "Fetch file content HTTP {} for {} @ {}: {}",
                status,
                file_path,
                commit_id,
                truncate_for_error(&body)
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| AppError::Ado(format!("Read file body failed: {}", e)))?;

        if debug {
            let preview: String = body.chars().take(160).collect::<String>().replace('\n', "\\n");
            eprintln!("[pex]   body: {} bytes; preview: {}", body.len(), preview);
        }

        // Decide envelope vs raw by Content-Type, NOT by sniffing the body —
        // a JSON *file's* content also starts with `{`, which used to make us
        // misread raw JSON files as ADO envelopes with a missing `content`
        // field, returning Empty for every file that starts with `{`.
        let is_json_envelope = content_type.starts_with("application/json");

        if is_json_envelope {
            if body.is_empty() {
                // JSON endpoint returned literally nothing — that's a fetch
                // anomaly, not a "file is empty" signal.
                return Ok(FetchOutcome::Inconclusive);
            }
            #[derive(serde::Deserialize)]
            struct ItemResp {
                #[serde(default)]
                content: Option<String>,
            }
            match serde_json::from_str::<ItemResp>(&body) {
                Ok(item) => match item.content {
                    Some(s) if !s.is_empty() => Ok(FetchOutcome::Text(s)),
                    // Empty string in the envelope means the file exists and is
                    // 0 bytes — a legitimate result for files like __init__.py.
                    Some(_) => Ok(FetchOutcome::Empty200),
                    // `content` was missing/null — ADO sometimes omits it for
                    // files past an internal size threshold. Caller retries.
                    None => Ok(FetchOutcome::Inconclusive),
                },
                Err(e) => Err(AppError::Ado(format!(
                    "Failed to parse ADO item envelope for {} @ {}: {}",
                    file_path, commit_id, e
                ))),
            }
        } else if body.is_empty() {
            // 200 with no body via the raw-text endpoint = the file exists
            // and is empty (zero bytes). Don't conflate with a fetch failure.
            Ok(FetchOutcome::Empty200)
        } else {
            // Raw bytes (application/octet-stream, text/plain, etc.) — this is
            // the file content verbatim.
            Ok(FetchOutcome::Text(body))
        }
    }

    // ---- Comments ----

    pub async fn get_threads(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
    ) -> Result<Vec<CommentThread>, AppError> {
        #[derive(serde::Deserialize)]
        struct Response {
            value: Vec<CommentThread>,
        }

        let resp: Response = self
            .get(&format!(
                "{}/_apis/git/repositories/{}/pullRequests/{}/threads?api-version={}",
                project, repo_id, pr_id, self.api_version
            ))
            .await?;
        Ok(resp.value)
    }

    pub async fn post_thread(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        thread: &serde_json::Value,
    ) -> Result<CommentThread, AppError> {
        self.post(
            &format!(
                "{}/_apis/git/repositories/{}/pullRequests/{}/threads?api-version={}",
                project, repo_id, pr_id, self.api_version
            ),
            thread,
        )
        .await
    }

    pub async fn add_comment_to_thread(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        thread_id: i64,
        comment: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        self.post(
            &format!(
                "{}/_apis/git/repositories/{}/pullRequests/{}/threads/{}/comments?api-version={}",
                project, repo_id, pr_id, thread_id, self.api_version
            ),
            comment,
        )
        .await
    }

    // ---- Reviewer Status ----

    pub async fn get_authenticated_user_id(&self) -> Result<String, AppError> {
        #[derive(serde::Deserialize)]
        struct ConnectionData {
            #[serde(rename = "authenticatedUser")]
            authenticated_user: AuthenticatedUser,
        }
        #[derive(serde::Deserialize)]
        struct AuthenticatedUser {
            id: String,
        }
        let resp: ConnectionData = self
            .get(&format!("_apis/connectionData?api-version={}", self.api_version))
            .await?;
        Ok(resp.authenticated_user.id)
    }

    pub async fn update_reviewer_status(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        reviewer_id: &str,
        vote: i32,
    ) -> Result<(), AppError> {
        let body = serde_json::json!({ "vote": vote });
        let _: serde_json::Value = self
            .patch(
                &format!(
                    "{}/_apis/git/repositories/{}/pullRequests/{}/reviewers/{}?api-version={}",
                    project, repo_id, pr_id, reviewer_id, self.api_version
                ),
                &body,
            )
            .await?;
        Ok(())
    }

    /// Public wrapper around the private `get_file_content` that returns a flat
    /// `Option<String>` — `None` for both missing files and inconclusive lookups.
    /// Used by the standards-context resolver, which prefers a conservative "absent"
    /// answer over surfacing a partial result that might mislead the model.
    pub async fn get_file_at_commit(
        &self,
        project: &str,
        repo_id: &str,
        commit_id: &str,
        file_path: &str,
    ) -> Result<Option<String>, AppError> {
        match self
            .get_file_content(project, repo_id, file_path, commit_id)
            .await?
        {
            FileSnapshot::Present(s) => Ok(Some(s)),
            FileSnapshot::Missing | FileSnapshot::Inconclusive => Ok(None),
        }
    }
}

/// Simple percent-encode for URL path segments
fn urlencoding(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('?', "%3F")
        .replace('+', "%2B")
        .replace('%', "%25")
}

/// Percent-encode a path for use as a query-string value, leaving '/' intact.
/// ADO's items endpoint accepts both forms but is more reliable with raw slashes.
fn encode_path_preserve_slash(s: &str) -> String {
    s.split('/')
        .map(urlencoding)
        .collect::<Vec<_>>()
        .join("/")
}

/// Cap an error-body snippet so we don't log megabytes of binary content.
fn truncate_for_error(s: &str) -> String {
    const MAX: usize = 240;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…(truncated {} bytes)", &s[..MAX], s.len() - MAX)
    }
}

/// Minimal HTML escape for short diagnostic strings.
fn escape_html_inline(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Outcome of one attempt to fetch a file's bytes from ADO.
enum FetchOutcome {
    /// 200 with a non-empty body that we interpret as the file content.
    Text(String),
    /// 200 with an empty body via the raw-text endpoint — the file exists but is empty.
    /// (`__init__.py`, `.gitkeep`, etc.) Treat as a real, empty file.
    Empty200,
    /// 404 — file legitimately doesn't exist at this commit (added/deleted file).
    Missing,
    /// 200 but the body didn't tell us what we needed (e.g. JSON envelope with
    /// `content: null` because ADO didn't inline content for this file).
    /// Caller should try a different fetch strategy.
    Inconclusive,
}

/// What we know about a file at one commit after exhausting our fetch strategies.
enum FileSnapshot {
    /// File exists at this commit; contents may be the empty string.
    Present(String),
    /// File does not exist at this commit.
    Missing,
    /// We never got a definitive answer (every strategy failed inconclusively).
    Inconclusive,
}

// ---- Response Types ----

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepoSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "defaultBranch")]
    pub default_branch: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PullRequest {
    #[serde(rename = "pullRequestId")]
    pub pull_request_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    // ADO returns drafts as status="active" with a separate isDraft flag, so
    // status alone never tells you a PR is in draft mode.
    #[serde(rename = "isDraft", default)]
    pub is_draft: bool,
    #[serde(rename = "createdBy")]
    pub created_by: IdentityRef,
    #[serde(rename = "sourceRefName")]
    pub source_ref_name: String,
    #[serde(rename = "targetRefName")]
    pub target_ref_name: String,
    #[serde(rename = "creationDate")]
    pub creation_date: String,
    #[serde(rename = "mergeStatus", default)]
    pub merge_status: Option<String>,
    #[serde(default)]
    pub reviewers: Vec<Reviewer>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IdentityRef {
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Reviewer {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub vote: i32,
    #[serde(rename = "isRequired", default)]
    pub is_required: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrFilesResult {
    pub files: Vec<FileChange>,
    pub commit_id: String,
    pub parent_commit_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Iteration {
    pub id: i64,
    pub name: String,
}

/// Trait for abstracting over Git providers (Azure DevOps, GitHub, etc.)
/// Used to enable future multi-provider support without rewriting the frontend.
#[allow(dead_code)]
pub trait GitProvider {
    fn list_pull_requests(
        &self,
    ) -> impl std::future::Future<Output = Result<Vec<PullRequest>, crate::AppError>>;
    fn get_file_diff(
        &self,
        pr_id: i64,
        path: &str,
        iteration: i32,
    ) -> impl std::future::Future<Output = Result<DiffResult, crate::AppError>>;
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileChange {
    #[serde(rename = "changeType")]
    pub change_type: String,
    pub item: FileItem,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileItem {
    pub path: String,
    #[serde(rename = "gitObjectType", default)]
    pub git_object_type: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffResult {
    pub html: String,
    pub path: String,
    pub status: String,
    #[serde(rename = "sourceCommit")]
    pub source_commit: String,
    #[serde(rename = "baseCommit")]
    pub base_commit: Option<String>,
    pub old_content: String,
    pub new_content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommentThread {
    pub id: i64,
    #[serde(rename = "threadContext", default)]
    pub thread_context: Option<ThreadContext>,
    pub status: Option<String>,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThreadContext {
    #[serde(rename = "filePath", default)]
    pub file_path: Option<String>,
    #[serde(rename = "rightFileStart", default)]
    pub right_file_start: Option<CommentPosition>,
    #[serde(rename = "rightFileEnd", default)]
    pub right_file_end: Option<CommentPosition>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommentPosition {
    pub line: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Comment {
    pub id: i64,
    pub author: Option<IdentityRef>,
    pub content: Option<String>,
    #[serde(rename = "publishedDate")]
    pub published_date: Option<String>,
}
