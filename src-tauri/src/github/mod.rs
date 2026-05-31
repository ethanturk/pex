//! GitHub REST API client. Mirrors `AdoClient`'s public surface and produces
//! the same provider-neutral structs (`crate::provider::model`). The owner
//! (org or user login) fills the "project" slot and the repo name fills the
//! "repo" slot; PR ids are GitHub PR `number`s.
//!
//! Auth is PAT-only for now (`Authorization: Bearer <pat>`). OAuth is not yet
//! supported (see `commands::auth`).

use crate::provider::model::*;
use crate::AppError;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use std::collections::{HashMap, HashSet};

const API_VERSION: &str = "2022-11-28";
const JSON_ACCEPT: &str = "application/vnd.github+json";
const RAW_ACCEPT: &str = "application/vnd.github.raw";

#[derive(Clone)]
pub struct GithubClient {
    /// Identity key for this connection (cache/keyring key). For github.com this
    /// is `https://github.com`; for GHES it's the server root `https://HOST`.
    org_url: String,
    /// REST API base, e.g. `https://api.github.com` or `https://HOST/api/v3`.
    api_base: String,
    auth_value: String, // "Bearer <pat>"
    http: reqwest::Client,
}

/// Derive the REST API base URL from a user-facing GitHub URL.
/// Blank or github.com → `https://api.github.com`; anything else is treated as
/// a GitHub Enterprise Server root → `https://HOST/api/v3`.
pub fn api_base_for(org_url: &str) -> String {
    let trimmed = org_url.trim().trim_end_matches('/');
    let host_only = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    if trimmed.is_empty() || host_only == "github.com" {
        "https://api.github.com".to_string()
    } else {
        format!("{}/api/v3", trimmed)
    }
}

/// Normalize a user-facing GitHub URL into the canonical identity key.
pub fn canonical_org_url(org_url: &str) -> String {
    let trimmed = org_url.trim().trim_end_matches('/');
    let host_only = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    if trimmed.is_empty() || host_only == "github.com" {
        "https://github.com".to_string()
    } else {
        trimmed.to_string()
    }
}

impl GithubClient {
    pub fn new(org_url: String, pat: String) -> Self {
        Self {
            api_base: api_base_for(&org_url),
            org_url: canonical_org_url(&org_url),
            auth_value: format!("Bearer {}", pat),
            http: reqwest::Client::new(),
        }
    }

    pub fn org_url(&self) -> &str {
        &self.org_url
    }

    fn headers(&self, accept: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(AUTHORIZATION, HeaderValue::from_str(&self.auth_value).unwrap());
        h.insert(ACCEPT, HeaderValue::from_str(accept).unwrap());
        h.insert(USER_AGENT, HeaderValue::from_static("pex"));
        h.insert(
            "X-GitHub-Api-Version",
            HeaderValue::from_static(API_VERSION),
        );
        h
    }

    /// Absolute URL for a path beginning with `/`.
    fn url(&self, path: &str) -> String {
        format!("{}{}", self.api_base, path)
    }

    async fn send(
        &self,
        method: Method,
        url: &str,
        accept: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<reqwest::Response, AppError> {
        let mut req = self.http.request(method, url).headers(self.headers(accept));
        if let Some(b) = body {
            req = req.json(b);
        }
        req.send()
            .await
            .map_err(|e| AppError::Provider(format!("GitHub request failed: {}", e)))
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, AppError> {
        let url = self.url(path);
        let resp = self.send(Method::GET, &url, JSON_ACCEPT, None).await?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| AppError::Provider(e.to_string()))?;
        if !status.is_success() {
            return Err(AppError::Provider(format!(
                "GitHub API {}: {}",
                status,
                truncate(&text)
            )));
        }
        serde_json::from_str(&text).map_err(|e| AppError::Provider(format!("Parse error: {}", e)))
    }

    async fn post_json<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, AppError> {
        let url = self.url(path);
        let resp = self.send(Method::POST, &url, JSON_ACCEPT, Some(body)).await?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| AppError::Provider(e.to_string()))?;
        if !status.is_success() {
            return Err(AppError::Provider(format!(
                "GitHub API {}: {}",
                status,
                truncate(&text)
            )));
        }
        serde_json::from_str(&text).map_err(|e| AppError::Provider(format!("Parse error: {}", e)))
    }

    /// GET every page of a paginated list endpoint, following `Link: rel="next"`.
    /// `path` must begin with `/` and include any query string (e.g. per_page).
    async fn get_paginated(&self, path: &str) -> Result<Vec<serde_json::Value>, AppError> {
        let mut out = Vec::new();
        let mut next = Some(self.url(path));
        while let Some(url) = next {
            let resp = self.send(Method::GET, &url, JSON_ACCEPT, None).await?;
            let status = resp.status();
            let link = resp
                .headers()
                .get(reqwest::header::LINK)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let text = resp.text().await.map_err(|e| AppError::Provider(e.to_string()))?;
            if !status.is_success() {
                return Err(AppError::Provider(format!(
                    "GitHub API {}: {}",
                    status,
                    truncate(&text)
                )));
            }
            let page: Vec<serde_json::Value> = serde_json::from_str(&text)
                .map_err(|e| AppError::Provider(format!("Parse error: {}", e)))?;
            out.extend(page);
            next = link.as_deref().and_then(parse_next_link);
        }
        Ok(out)
    }

    // ---- Auth / user ----

    pub async fn get_authenticated_user_id(&self) -> Result<String, AppError> {
        let me: serde_json::Value = self.get_json("/user").await?;
        Ok(me
            .get("login")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string())
    }

    // ---- Owners (projects) & repos ----

    pub async fn list_projects(&self) -> Result<Vec<ProjectSummary>, AppError> {
        let me: serde_json::Value = self.get_json("/user").await?;
        let login = me
            .get("login")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let mut out = vec![ProjectSummary {
            id: login.clone(),
            name: login,
        }];
        let orgs = self.get_paginated("/user/orgs?per_page=100").await?;
        for o in orgs {
            if let Some(name) = o.get("login").and_then(|v| v.as_str()) {
                out.push(ProjectSummary {
                    id: name.to_string(),
                    name: name.to_string(),
                });
            }
        }
        Ok(out)
    }

    pub async fn list_repositories(&self, owner: &str) -> Result<Vec<RepoSummary>, AppError> {
        // Decide between the authed user's repos and an org's repos.
        let me: serde_json::Value = self.get_json("/user").await?;
        let my_login = me.get("login").and_then(|v| v.as_str()).unwrap_or_default();
        let raw = if owner.eq_ignore_ascii_case(my_login) {
            self.get_paginated(
                "/user/repos?affiliation=owner,collaborator,organization_member&per_page=100&sort=updated",
            )
            .await?
        } else {
            self.get_paginated(&format!("/orgs/{}/repos?per_page=100&sort=updated", owner))
                .await?
        };
        Ok(raw
            .into_iter()
            .map(|r| {
                let name = r
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let default_branch = r
                    .get("default_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main")
                    .to_string();
                RepoSummary {
                    id: name.clone(),
                    name,
                    default_branch,
                }
            })
            .collect())
    }

    // ---- Pull requests ----

    pub async fn list_pull_requests(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<PullRequest>, AppError> {
        let raw = self
            .get_paginated(&format!(
                "/repos/{}/{}/pulls?state=open&per_page=100&sort=updated&direction=desc",
                owner, repo
            ))
            .await?;
        // List view: cheap mapping without per-PR review fetches.
        Ok(raw.iter().map(|pr| map_pull_request(pr, &[])).collect())
    }

    pub async fn get_pull_request(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<PullRequest, AppError> {
        let pr: serde_json::Value = self
            .get_json(&format!("/repos/{}/{}/pulls/{}", owner, repo, number))
            .await?;
        // Enrich reviewer votes from the reviews endpoint.
        let reviews = self
            .get_paginated(&format!(
                "/repos/{}/{}/pulls/{}/reviews?per_page=100",
                owner, repo, number
            ))
            .await
            .unwrap_or_default();
        Ok(map_pull_request(&pr, &reviews))
    }

    /// Fetch (head_sha, base_sha) for a PR.
    async fn pr_commits(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<(String, String), AppError> {
        let pr: serde_json::Value = self
            .get_json(&format!("/repos/{}/{}/pulls/{}", owner, repo, number))
            .await?;
        let head = pr
            .pointer("/head/sha")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let base = pr
            .pointer("/base/sha")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        Ok((head, base))
    }

    /// Merge-base of base..head, so diffs match GitHub's 3-dot PR view.
    async fn merge_base(
        &self,
        owner: &str,
        repo: &str,
        base_sha: &str,
        head_sha: &str,
    ) -> Result<String, AppError> {
        let cmp: serde_json::Value = self
            .get_json(&format!(
                "/repos/{}/{}/compare/{}...{}?per_page=1",
                owner, repo, base_sha, head_sha
            ))
            .await?;
        Ok(cmp
            .pointer("/merge_base_commit/sha")
            .and_then(|v| v.as_str())
            .unwrap_or(base_sha)
            .to_string())
    }

    pub async fn list_pr_policy_evaluations(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<Vec<PrCheck>, AppError> {
        let (head_sha, _) = self.pr_commits(owner, repo, number).await?;
        if head_sha.is_empty() {
            return Ok(Vec::new());
        }
        let resp: serde_json::Value = self
            .get_json(&format!(
                "/repos/{}/{}/commits/{}/check-runs?per_page=100",
                owner, repo, head_sha
            ))
            .await?;
        let runs = resp
            .get("check_runs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(runs.iter().map(map_check_run).collect())
    }

    pub async fn get_iterations(
        &self,
        _owner: &str,
        _repo: &str,
        _number: i64,
    ) -> Result<Vec<Iteration>, AppError> {
        // GitHub has no iteration concept; expose a single synthetic iteration.
        Ok(vec![Iteration {
            id: 1,
            name: Some("Latest".to_string()),
        }])
    }

    pub async fn changed_paths_since_iteration(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        _from_iteration: i32,
        _to_iteration: i32,
    ) -> Result<HashSet<String>, AppError> {
        // No iterations → treat the whole changeset as "changed".
        let files = self.get_pr_files(owner, repo, number, 1).await?;
        Ok(files
            .files
            .into_iter()
            .map(|f| f.item.path.trim_start_matches('/').to_string())
            .filter(|p| !p.is_empty())
            .collect())
    }

    // ---- Files & diffs ----

    pub async fn get_pr_files(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        _iteration: i32,
    ) -> Result<PrFilesResult, AppError> {
        let (head_sha, base_sha) = self.pr_commits(owner, repo, number).await?;
        let merge_base = self
            .merge_base(owner, repo, &base_sha, &head_sha)
            .await
            .unwrap_or_else(|_| base_sha.clone());
        let raw = self
            .get_paginated(&format!(
                "/repos/{}/{}/pulls/{}/files?per_page=100",
                owner, repo, number
            ))
            .await?;
        let files = raw
            .iter()
            .map(|f| FileChange {
                change_type: map_file_status(
                    f.get("status").and_then(|v| v.as_str()).unwrap_or("modified"),
                )
                .to_string(),
                item: FileItem {
                    path: f
                        .get("filename")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    git_object_type: Some("blob".to_string()),
                },
            })
            .collect();
        Ok(PrFilesResult {
            files,
            commit_id: head_sha,
            parent_commit_id: Some(merge_base),
        })
    }

    pub async fn get_file_diff(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        file_path: &str,
        _iteration: i32,
        view: crate::diff::engine::DiffView,
    ) -> Result<DiffResult, AppError> {
        let (head_sha, base_sha) = self.pr_commits(owner, repo, number).await?;
        let merge_base = self
            .merge_base(owner, repo, &base_sha, &head_sha)
            .await
            .unwrap_or_else(|_| base_sha.clone());

        let new_content = self
            .get_content(owner, repo, file_path, &head_sha)
            .await?;
        let old_content = self
            .get_content(owner, repo, file_path, &merge_base)
            .await?;

        let old_str = old_content.clone().unwrap_or_default();
        let new_str = new_content.clone().unwrap_or_default();

        let html =
            crate::diff::engine::highlighted_diff_view(&old_str, &new_str, file_path, view);

        let change_type = match (&old_content, &new_content) {
            (None, _) => "add",
            (_, None) => "delete",
            _ => "edit",
        };

        Ok(DiffResult {
            html,
            path: file_path.to_string(),
            status: change_type.to_string(),
            source_commit: head_sha,
            base_commit: Some(merge_base),
            old_content: old_str,
            new_content: new_str,
        })
    }

    pub async fn get_file_lines(
        &self,
        owner: &str,
        repo: &str,
        commit_id: &str,
        file_path: &str,
        start_line: usize,
        end_line: usize,
    ) -> Result<Vec<String>, AppError> {
        if start_line == 0 || end_line < start_line {
            return Ok(Vec::new());
        }
        let content = match self.get_content(owner, repo, file_path, commit_id).await? {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };
        Ok(content
            .lines()
            .skip(start_line - 1)
            .take(end_line - start_line + 1)
            .map(|s| s.to_string())
            .collect())
    }

    pub async fn get_file_at_commit(
        &self,
        owner: &str,
        repo: &str,
        commit_id: &str,
        file_path: &str,
    ) -> Result<Option<String>, AppError> {
        self.get_content(owner, repo, file_path, commit_id).await
    }

    /// Fetch raw file content at a commit. `Ok(None)` means the file doesn't
    /// exist at that ref (404) — i.e. added/deleted on one side.
    async fn get_content(
        &self,
        owner: &str,
        repo: &str,
        file_path: &str,
        commit_id: &str,
    ) -> Result<Option<String>, AppError> {
        let path = file_path.trim_start_matches('/');
        let encoded = encode_path_preserve_slash(path);
        let url = self.url(&format!(
            "/repos/{}/{}/contents/{}?ref={}",
            owner, repo, encoded, commit_id
        ));
        let resp = self.send(Method::GET, &url, RAW_ACCEPT, None).await?;
        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let text = resp.text().await.map_err(|e| AppError::Provider(e.to_string()))?;
        if !status.is_success() {
            return Err(AppError::Provider(format!(
                "GitHub content {} for {} @ {}: {}",
                status,
                file_path,
                commit_id,
                truncate(&text)
            )));
        }
        Ok(Some(text))
    }

    // ---- Comments ----

    pub async fn get_threads(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
    ) -> Result<Vec<CommentThread>, AppError> {
        let review_comments = self
            .get_paginated(&format!(
                "/repos/{}/{}/pulls/{}/comments?per_page=100",
                owner, repo, number
            ))
            .await?;
        let issue_comments = self
            .get_paginated(&format!(
                "/repos/{}/{}/issues/{}/comments?per_page=100",
                owner, repo, number
            ))
            .await?;
        Ok(build_threads(&review_comments, &issue_comments))
    }

    pub async fn post_thread(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        thread: &serde_json::Value,
    ) -> Result<CommentThread, AppError> {
        let content = thread
            .pointer("/comments/0/content")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let file_path = thread
            .pointer("/threadContext/filePath")
            .and_then(|v| v.as_str())
            .map(|p| p.trim_start_matches('/').to_string());
        let line = thread
            .pointer("/threadContext/rightFileStart/line")
            .and_then(|v| v.as_i64());

        match (file_path, line) {
            // Anchored review comment.
            (Some(path), Some(line)) => {
                let (head_sha, _) = self.pr_commits(owner, repo, number).await?;
                let body = serde_json::json!({
                    "body": content,
                    "commit_id": head_sha,
                    "path": path,
                    "line": line,
                    "side": "RIGHT",
                });
                let resp: serde_json::Value = self
                    .post_json(
                        &format!("/repos/{}/{}/pulls/{}/comments", owner, repo, number),
                        &body,
                    )
                    .await?;
                Ok(review_comment_to_thread(&resp))
            }
            // File-level or PR-level → issue comment (GitHub review comments
            // require a line). Prefix the path when we have one.
            (maybe_path, _) => {
                let body_text = match &maybe_path {
                    Some(p) => format!("**{}**\n\n{}", p, content),
                    None => content,
                };
                let body = serde_json::json!({ "body": body_text });
                let resp: serde_json::Value = self
                    .post_json(
                        &format!("/repos/{}/{}/issues/{}/comments", owner, repo, number),
                        &body,
                    )
                    .await?;
                Ok(issue_comment_to_thread(&resp))
            }
        }
    }

    pub async fn add_comment_to_thread(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        thread_id: i64,
        comment: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let content = comment
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let body = serde_json::json!({ "body": content });
        let resp: serde_json::Value = self
            .post_json(
                &format!(
                    "/repos/{}/{}/pulls/{}/comments/{}/replies",
                    owner, repo, number, thread_id
                ),
                &body,
            )
            .await?;
        // Shape the response so `commands::comments::post_reply` can read it.
        Ok(serde_json::json!({
            "id": resp.get("id").cloned().unwrap_or(serde_json::Value::Null),
            "author": { "displayName": resp.pointer("/user/login").and_then(|v| v.as_str()).unwrap_or("") },
            "content": resp.get("body").and_then(|v| v.as_str()).unwrap_or(""),
            "publishedDate": resp.get("created_at").and_then(|v| v.as_str()).unwrap_or(""),
        }))
    }

    pub async fn update_reviewer_status(
        &self,
        owner: &str,
        repo: &str,
        number: i64,
        _reviewer_id: &str,
        vote: i32,
    ) -> Result<(), AppError> {
        let (event, body_text) = vote_to_event(vote);
        let mut body = serde_json::json!({ "event": event });
        if let Some(text) = body_text {
            body["body"] = serde_json::Value::String(text.to_string());
        }
        let _: serde_json::Value = self
            .post_json(
                &format!("/repos/{}/{}/pulls/{}/reviews", owner, repo, number),
                &body,
            )
            .await?;
        Ok(())
    }
}

// ---- Pure mappers (unit-tested) ----

/// Map a GitHub PR `files[].status` to the app's change-type vocabulary.
fn map_file_status(status: &str) -> &'static str {
    match status {
        "added" => "add",
        "removed" => "delete",
        "renamed" => "rename",
        _ => "edit", // modified, changed, copied, unchanged
    }
}

/// Map the app's integer vote to a GitHub review event (and an optional body,
/// required by GitHub for non-APPROVE events).
fn vote_to_event(vote: i32) -> (&'static str, Option<&'static str>) {
    match vote {
        v if v >= 10 => ("APPROVE", None),
        v if v <= -10 => ("REQUEST_CHANGES", Some("Changes requested.")),
        _ => ("COMMENT", Some("Comment.")),
    }
}

/// Map a GitHub review state to the app's integer vote.
fn review_state_to_vote(state: &str) -> i32 {
    match state.to_ascii_uppercase().as_str() {
        "APPROVED" => 10,
        "CHANGES_REQUESTED" => -10,
        _ => 0, // COMMENTED, DISMISSED, PENDING
    }
}

fn map_pull_request(pr: &serde_json::Value, reviews: &[serde_json::Value]) -> PullRequest {
    let s = |ptr: &str| pr.pointer(ptr).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let login = pr
        .pointer("/user/login")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let state = pr.get("state").and_then(|v| v.as_str()).unwrap_or("open");
    let merged = pr.get("merged_at").map(|v| !v.is_null()).unwrap_or(false);
    let status = match (state, merged) {
        ("open", _) => "active",
        (_, true) => "completed",
        _ => "abandoned",
    };

    PullRequest {
        pull_request_id: pr.get("number").and_then(|v| v.as_i64()).unwrap_or(0),
        title: s("/title"),
        description: pr.get("body").and_then(|v| v.as_str()).map(|x| x.to_string()),
        status: status.to_string(),
        is_draft: pr.get("draft").and_then(|v| v.as_bool()).unwrap_or(false),
        created_by: IdentityRef {
            display_name: login.clone(),
            id: login,
        },
        source_ref_name: format!("refs/heads/{}", s("/head/ref")),
        target_ref_name: format!("refs/heads/{}", s("/base/ref")),
        creation_date: s("/created_at"),
        merge_status: pr
            .get("mergeable_state")
            .and_then(|v| v.as_str())
            .map(|x| x.to_string()),
        reviewers: build_reviewers(pr, reviews),
    }
}

/// Combine requested reviewers (no vote yet) with submitted review states,
/// keyed by login. The latest meaningful review state wins per user.
fn build_reviewers(pr: &serde_json::Value, reviews: &[serde_json::Value]) -> Vec<Reviewer> {
    let mut votes: HashMap<String, i32> = HashMap::new();

    for r in reviews {
        let login = r
            .pointer("/user/login")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if login.is_empty() {
            continue;
        }
        let state = r.get("state").and_then(|v| v.as_str()).unwrap_or("");
        let vote = review_state_to_vote(state);
        // Reviews are returned chronologically; a later APPROVED/CHANGES_REQUESTED
        // supersedes an earlier one, while bare COMMENTED (0) shouldn't clobber a vote.
        let entry = votes.entry(login.to_string()).or_insert(0);
        if vote != 0 {
            *entry = vote;
        }
    }

    // Requested reviewers who haven't voted yet.
    if let Some(reqd) = pr.get("requested_reviewers").and_then(|v| v.as_array()) {
        for rv in reqd {
            if let Some(login) = rv.get("login").and_then(|v| v.as_str()) {
                votes.entry(login.to_string()).or_insert(0);
            }
        }
    }

    let mut out: Vec<Reviewer> = votes
        .into_iter()
        .map(|(login, vote)| Reviewer {
            id: login.clone(),
            display_name: login,
            vote,
            is_required: false,
        })
        .collect();
    out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    out
}

fn map_check_run(run: &serde_json::Value) -> PrCheck {
    let id = run
        .get("id")
        .map(|v| v.to_string())
        .unwrap_or_default();
    // GitHub: status is queued/in_progress/completed; conclusion is the result
    // once completed (success/failure/neutral/…). Prefer the conclusion.
    let status = run
        .get("conclusion")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| run.get("status").and_then(|v| v.as_str()))
        .unwrap_or("queued")
        .to_string();
    PrCheck {
        id,
        name: run
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Check")
            .to_string(),
        status,
        is_required: false,
        description: run
            .pointer("/output/title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        started_date: run
            .get("started_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        completed_date: run
            .get("completed_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

fn json_comment(c: &serde_json::Value) -> Comment {
    Comment {
        id: c.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        author: Some(IdentityRef {
            display_name: c
                .pointer("/user/login")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            id: c
                .pointer("/user/login")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        content: c.get("body").and_then(|v| v.as_str()).map(|s| s.to_string()),
        published_date: c
            .get("created_at")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        is_deleted: false,
    }
}

fn review_comment_to_thread(c: &serde_json::Value) -> CommentThread {
    let line = c
        .get("line")
        .and_then(|v| v.as_i64())
        .or_else(|| c.get("original_line").and_then(|v| v.as_i64()))
        .unwrap_or(0);
    let path = c.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
    CommentThread {
        id: c.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        thread_context: Some(ThreadContext {
            file_path: Some(path),
            right_file_start: Some(CommentPosition { line, offset: 1 }),
            right_file_end: Some(CommentPosition { line, offset: 1 }),
        }),
        status: None,
        comments: vec![json_comment(c)],
        is_deleted: false,
    }
}

fn issue_comment_to_thread(c: &serde_json::Value) -> CommentThread {
    CommentThread {
        id: c.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        thread_context: None,
        status: None,
        comments: vec![json_comment(c)],
        is_deleted: false,
    }
}

/// Group GitHub review comments into threads by their root comment id (following
/// `in_reply_to_id` chains), then append PR-level issue comments as singletons.
fn build_threads(
    review_comments: &[serde_json::Value],
    issue_comments: &[serde_json::Value],
) -> Vec<CommentThread> {
    // id -> in_reply_to_id (immediate parent)
    let parent: HashMap<i64, i64> = review_comments
        .iter()
        .filter_map(|c| {
            let id = c.get("id").and_then(|v| v.as_i64())?;
            let p = c.get("in_reply_to_id").and_then(|v| v.as_i64())?;
            Some((id, p))
        })
        .collect();

    let root_of = |mut id: i64| -> i64 {
        let mut guard = 0;
        while let Some(&p) = parent.get(&id) {
            id = p;
            guard += 1;
            if guard > 10_000 {
                break;
            }
        }
        id
    };

    // root id -> thread (preserving first-seen order)
    let mut order: Vec<i64> = Vec::new();
    let mut threads: HashMap<i64, CommentThread> = HashMap::new();

    for c in review_comments {
        let id = c.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let root = root_of(id);
        let entry = threads.entry(root).or_insert_with(|| {
            order.push(root);
            // Seed thread metadata from whichever comment is the root if present,
            // else from this comment.
            review_comment_to_thread(c)
        });
        // The seed already added the root comment's body when c == root; for
        // non-root comments (or when the root comment object differs), append.
        if id != root || entry.comments.is_empty() {
            entry.comments.push(json_comment(c));
        } else if entry.id == 0 {
            entry.id = root;
        }
    }

    // Dedup: the seed for the root may double-add the root comment. Rebuild
    // each thread's comment list cleanly, keyed by comment id, sorted by date.
    let mut out: Vec<CommentThread> = Vec::new();
    for root in &order {
        if let Some(t) = threads.get(root) {
            let mut seen = HashSet::new();
            let mut comments: Vec<Comment> = Vec::new();
            for c in &t.comments {
                if seen.insert(c.id) {
                    comments.push(c.clone());
                }
            }
            comments.sort_by(|a, b| a.published_date.cmp(&b.published_date));
            let mut t2 = t.clone();
            t2.id = *root;
            t2.comments = comments;
            out.push(t2);
        }
    }

    for c in issue_comments {
        out.push(issue_comment_to_thread(c));
    }
    out
}

// ---- helpers ----

/// Parse the `next` URL out of a GitHub `Link` header value.
fn parse_next_link(link: &str) -> Option<String> {
    for part in link.split(',') {
        let part = part.trim();
        if part.contains("rel=\"next\"") {
            let start = part.find('<')?;
            let end = part.find('>')?;
            return Some(part[start + 1..end].to_string());
        }
    }
    None
}

fn encode_path_preserve_slash(s: &str) -> String {
    s.split('/')
        .map(|seg| {
            seg.replace('%', "%25")
                .replace(' ', "%20")
                .replace('#', "%23")
                .replace('?', "%3F")
                .replace('+', "%2B")
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn truncate(s: &str) -> String {
    const MAX: usize = 240;
    if s.len() <= MAX {
        s.to_string()
    } else {
        format!("{}…(truncated {} bytes)", &s[..MAX], s.len() - MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_status_mapping() {
        assert_eq!(map_file_status("added"), "add");
        assert_eq!(map_file_status("removed"), "delete");
        assert_eq!(map_file_status("renamed"), "rename");
        assert_eq!(map_file_status("modified"), "edit");
        assert_eq!(map_file_status("copied"), "edit");
    }

    #[test]
    fn vote_event_mapping() {
        assert_eq!(vote_to_event(10).0, "APPROVE");
        assert_eq!(vote_to_event(-10).0, "REQUEST_CHANGES");
        assert_eq!(vote_to_event(5).0, "COMMENT");
        assert_eq!(vote_to_event(-5).0, "COMMENT");
        // Non-APPROVE events must carry a body (GitHub requires it).
        assert!(vote_to_event(-10).1.is_some());
        assert!(vote_to_event(5).1.is_some());
        assert!(vote_to_event(10).1.is_none());
    }

    #[test]
    fn review_state_mapping() {
        assert_eq!(review_state_to_vote("APPROVED"), 10);
        assert_eq!(review_state_to_vote("CHANGES_REQUESTED"), -10);
        assert_eq!(review_state_to_vote("COMMENTED"), 0);
    }

    #[test]
    fn api_base_derivation() {
        assert_eq!(api_base_for(""), "https://api.github.com");
        assert_eq!(api_base_for("https://github.com"), "https://api.github.com");
        assert_eq!(api_base_for("https://github.com/"), "https://api.github.com");
        assert_eq!(
            api_base_for("https://ghe.example.com"),
            "https://ghe.example.com/api/v3"
        );
    }

    #[test]
    fn next_link_parsing() {
        let link = "<https://api.github.com/x?page=2>; rel=\"next\", <https://api.github.com/x?page=5>; rel=\"last\"";
        assert_eq!(
            parse_next_link(link).as_deref(),
            Some("https://api.github.com/x?page=2")
        );
        assert_eq!(parse_next_link("<...>; rel=\"last\""), None);
    }

    #[test]
    fn threads_group_replies_by_root() {
        let review = serde_json::json!([
            { "id": 1, "path": "a.rs", "line": 5, "body": "root", "user": {"login": "alice"}, "created_at": "2024-01-01T00:00:00Z" },
            { "id": 2, "in_reply_to_id": 1, "path": "a.rs", "line": 5, "body": "reply", "user": {"login": "bob"}, "created_at": "2024-01-02T00:00:00Z" },
            { "id": 3, "path": "b.rs", "line": 9, "body": "other", "user": {"login": "carol"}, "created_at": "2024-01-03T00:00:00Z" },
        ]);
        let issue = serde_json::json!([
            { "id": 100, "body": "pr-level", "user": {"login": "dave"}, "created_at": "2024-01-04T00:00:00Z" },
        ]);
        let threads = build_threads(
            review.as_array().unwrap(),
            issue.as_array().unwrap(),
        );
        // 2 review threads (root 1 with reply, root 3) + 1 issue thread.
        assert_eq!(threads.len(), 3);
        let t1 = threads.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(t1.comments.len(), 2);
        assert_eq!(t1.comments[0].content.as_deref(), Some("root"));
        assert_eq!(t1.comments[1].content.as_deref(), Some("reply"));
        let issue_thread = threads.iter().find(|t| t.id == 100).unwrap();
        assert!(issue_thread.thread_context.is_none());
    }
}
