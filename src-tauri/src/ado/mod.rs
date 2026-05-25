use crate::AppError;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use base64::Engine;
use serde::de::DeserializeOwned;

/// Azure DevOps REST API client.
#[derive(Clone)]
pub struct AdoClient {
    pub org_url: String,
    auth_value: String,  // "Basic base64" or "Bearer token"
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
        let body = resp.text().await.map_err(|e| AppError::Ado(e.to_string()))?;

        if !status.is_success() {
            return Err(AppError::Ado(format!("ADO API {}: {}", status, body)));
        }

        serde_json::from_str(&body).map_err(|e| AppError::Ado(format!("Parse error: {}", e)))
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: &serde_json::Value) -> Result<T, AppError> {
        let url = format!("{}/{}", self.org_url, path);
        let resp = self
            .http
            .post(&url)
            .headers(self.auth_headers())
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| AppError::Ado(e.to_string()))?;

        if !status.is_success() {
            return Err(AppError::Ado(format!("ADO API {}: {}", status, text)));
        }

        serde_json::from_str(&text).map_err(|e| AppError::Ado(format!("Parse error: {}", e)))
    }

    async fn patch<T: DeserializeOwned>(&self, path: &str, body: &serde_json::Value) -> Result<T, AppError> {
        let url = format!("{}/{}", self.org_url, path);
        let resp = self
            .http
            .patch(&url)
            .headers(self.auth_headers())
            .json(body)
            .send()
            .await?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| AppError::Ado(e.to_string()))?;

        if !status.is_success() {
            return Err(AppError::Ado(format!("ADO API {}: {}", status, text)));
        }

        serde_json::from_str(&text).map_err(|e| AppError::Ado(format!("Parse error: {}", e)))
    }

    // ---- Projects & Repos ----

    pub async fn list_projects(&self) -> Result<Vec<ProjectSummary>, AppError> {
        #[derive(serde::Deserialize)]
        struct Response { value: Vec<ProjectSummary> }
        let resp: Response = self.get(&format!(
            "_apis/projects?api-version={}",
            self.api_version
        )).await?;
        Ok(resp.value)
    }

    pub async fn list_repositories(&self, project: &str) -> Result<Vec<RepoSummary>, AppError> {
        #[derive(serde::Deserialize)]
        struct Response { value: Vec<RepoSummary> }
        let resp: Response = self.get(&format!(
            "{}/_apis/git/repositories?api-version={}",
            project, self.api_version
        )).await?;
        Ok(resp.value)
    }

    // ---- Pull Requests ----

    pub async fn list_pull_requests(
        &self,
        project: &str,
        repo_id: &str,
    ) -> Result<Vec<PullRequest>, AppError> {
        #[derive(serde::Deserialize)]
        struct Response { value: Vec<PullRequest> }

        let resp: Response = self.get(&format!(
            "{}/_apis/git/repositories/{}/pullrequests?searchCriteria.status=active&api-version={}",
            project, repo_id, self.api_version
        )).await?;
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
        )).await
    }

    // ---- Files & Diffs ----

    pub async fn get_pr_files(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        iteration: i32,
    ) -> Result<PrFilesResult, AppError> {
        #[derive(serde::Deserialize)]
        struct CommitResponse {
            #[serde(rename = "commitId")]
            commit_id: String,
            #[serde(rename = "parents")]
            parents: Vec<String>,
        }
        #[derive(serde::Deserialize)]
        struct CommitsResponse { value: Vec<CommitResponse> }

        let commits: CommitsResponse = self.get(&format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/commits?$top=1&iteration={}&api-version={}",
            project, repo_id, pr_id, iteration, self.api_version
        )).await?;

        let commit = commits
            .value
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Ado("No commits found for iteration".into()))?;

        let parent_commit_id = commit.parents.into_iter().next();

        #[derive(serde::Deserialize)]
        struct ChangesResponse { changes: Vec<FileChange> }

        let changes: ChangesResponse = self.get(&format!(
            "{}/_apis/git/repositories/{}/commits/{}/changes?$top=1000&api-version={}",
            project, repo_id, commit.commit_id, self.api_version
        )).await?;

        Ok(PrFilesResult {
            files: changes.changes,
            commit_id: commit.commit_id,
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
        // 1. Get the iteration commit + parent
        #[derive(serde::Deserialize)]
        struct CommitResp {
            #[serde(rename = "commitId")]
            commit_id: String,
            #[serde(rename = "parents")]
            parents: Vec<String>,
        }
        #[derive(serde::Deserialize)]
        struct CommitsList { value: Vec<CommitResp> }

        let commits: CommitsList = self.get(&format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/commits?$top=1&iteration={}&api-version={}",
            project, repo_id, pr_id, iteration, self.api_version
        )).await?;

        let commit = commits.value.into_iter().next()
            .ok_or_else(|| AppError::Ado("No commits found for iteration".into()))?;
        let parent_id = commit.parents.into_iter().next();

        // 2. Fetch new file content (at the iteration commit)
        let new_content = self.get_file_content(project, repo_id, file_path, &commit.commit_id).await;

        // 3. Fetch old file content (at the parent commit, or empty for new files)
        let old_content = match &parent_id {
            Some(pid) => self.get_file_content(project, repo_id, file_path, pid).await,
            None => String::new(), // No parent = initial commit, all files are new
        };

        // 4. Compute diff + syntax highlight
        let html = crate::diff::engine::highlighted_diff(&old_content, &new_content, file_path);

        let change_type = if old_content.is_empty() { "add" }
            else if new_content.is_empty() { "delete" }
            else { "edit" };

        Ok(DiffResult {
            html,
            path: file_path.to_string(),
            status: change_type.to_string(),
        })
    }

    /// Fetch raw file content at a specific commit version.
    async fn get_file_content(
        &self,
        project: &str,
        repo_id: &str,
        file_path: &str,
        commit_id: &str,
    ) -> String {
        let url = format!(
            "{}/{}/_apis/git/repositories/{}/items?path={}&versionDescriptor.version={}&api-version={}",
            self.org_url, project, repo_id,
            urlencoding(file_path),
            commit_id,
            self.api_version
        );

        match self.http.get(&url).headers(self.auth_headers()).send().await {
            Ok(resp) if resp.status().is_success() => {
                resp.text().await.unwrap_or_default()
            }
            _ => String::new(), // File doesn't exist at this version (new or deleted)
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
        struct _Response { value: Vec<CommentThread> }

        self.get(&format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/threads?api-version={}",
            project, repo_id, pr_id, self.api_version
        )).await
    }

    pub async fn post_thread(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        thread: &serde_json::Value,
    ) -> Result<CommentThread, AppError> {
        self.post(&format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/threads?api-version={}",
            project, repo_id, pr_id, self.api_version
        ), thread).await
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
        let _: serde_json::Value = self.patch(&format!(
            "{}/_apis/git/repositories/{}/pullRequests/{}/reviewers/{}?api-version={}",
            project, repo_id, pr_id, reviewer_id, self.api_version
        ), &body).await?;
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
    #[serde(rename = "mergeStatus")]
    pub merge_status: String,
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
    pub vote: i32,
    #[serde(rename = "isRequired")]
    pub is_required: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrFilesResult {
    pub files: Vec<FileChange>,
    pub commit_id: String,
    pub parent_commit_id: Option<String>,
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
    #[serde(rename = "gitObjectType")]
    pub git_object_type: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffResult {
    pub html: String,
    pub path: String,
    pub status: String,
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
