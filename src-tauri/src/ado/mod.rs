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
    ) -> Result<DiffResult, AppError> {
        // Diff the iteration HEAD (sourceRefCommit) against the PR merge base (commonRefCommit),
        // not the latest source-branch commit's parent (which would give per-commit diffs).
        let (source_commit, base_commit) = self
            .get_iteration_detail(project, repo_id, pr_id, iteration)
            .await?;

        let new_content = self
            .get_file_content(project, repo_id, file_path, &source_commit)
            .await?;

        let old_content = match &base_commit {
            Some(bid) => self
                .get_file_content(project, repo_id, file_path, bid)
                .await?,
            None => String::new(),
        };

        // 4. Compute diff + syntax highlight
        let html = crate::diff::engine::highlighted_diff(&old_content, &new_content, file_path);

        let change_type = if old_content.is_empty() {
            "add"
        } else if new_content.is_empty() {
            "delete"
        } else {
            "edit"
        };

        Ok(DiffResult {
            html,
            path: file_path.to_string(),
            status: change_type.to_string(),
            source_commit,
            base_commit,
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
        let content = self
            .get_file_content(project, repo_id, file_path, commit_id)
            .await?;
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
    ) -> Result<String, AppError> {
        // ADO's items endpoint can return JSON metadata or raw bytes depending on
        // $format AND the Accept header. We request the JSON envelope with
        // includeContent=true and pull the `content` field — this is unambiguous
        // regardless of what the server decides about Accept negotiation.
        // versionType=commit is required — without it ADO defaults to "branch" and
        // tries to resolve the SHA as a branch name (always 404).
        // ADO's items endpoint expects the path without a URL-encoded leading slash
        // in some configurations; normalize by stripping a leading '/'.
        let normalized_path = file_path.strip_prefix('/').unwrap_or(file_path);
        let url = format!(
            "{}/{}/_apis/git/repositories/{}/items?path=/{}&versionDescriptor.version={}&versionDescriptor.versionType=commit&includeContent=true&api-version={}",
            self.org_url, project, repo_id,
            urlencoding(normalized_path),
            commit_id,
            self.api_version
        );

        let mut headers = self.auth_headers();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        let resp = self
            .http
            .get(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| AppError::Ado(format!("Fetch file content failed: {}", e)))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            // File doesn't exist at this version — added or deleted file.
            return Ok(String::new());
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Ado(format!(
                "Fetch file content HTTP {} for {} @ {}: {}",
                status, file_path, commit_id, body
            )));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| AppError::Ado(format!("Read file body failed: {}", e)))?;

        // Body may be either JSON metadata (Accept honored) or raw text (server ignored Accept).
        // Heuristic: JSON shape starts with `{` and contains `"content"`.
        if body.starts_with('{') {
            #[derive(serde::Deserialize)]
            struct ItemResp {
                #[serde(default)]
                content: Option<String>,
            }
            match serde_json::from_str::<ItemResp>(&body) {
                Ok(item) => Ok(item.content.unwrap_or_default()),
                // Fall through: not the shape we expected, treat the body as raw text.
                Err(_) => Ok(body),
            }
        } else {
            Ok(body)
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
