//! Provider abstraction. `GitClient` is an enum over the concrete provider
//! clients (Azure DevOps, GitHub). It exposes the full method surface the rest
//! of the app calls and dispatches each call to the active provider. An enum is
//! used rather than a trait object because the client is cloned out of a mutex
//! and moved into `tokio::spawn`, where `Clone`/`Send` come for free and async
//! methods keep `impl Future` returns with no extra dependencies.

pub mod model;

pub use model::*;

use crate::AppError;
use std::collections::HashSet;

#[derive(Clone)]
pub enum GitClient {
    Ado(crate::ado::AdoClient),
    Github(crate::github::GithubClient),
}

impl GitClient {
    /// Base URL identifying the connection (used as a cache/keyring key).
    pub fn org_url(&self) -> &str {
        match self {
            GitClient::Ado(c) => c.org_url.as_str(),
            GitClient::Github(c) => c.org_url(),
        }
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectSummary>, AppError> {
        match self {
            GitClient::Ado(c) => c.list_projects().await,
            GitClient::Github(c) => c.list_projects().await,
        }
    }

    pub async fn list_repositories(&self, project: &str) -> Result<Vec<RepoSummary>, AppError> {
        match self {
            GitClient::Ado(c) => c.list_repositories(project).await,
            GitClient::Github(c) => c.list_repositories(project).await,
        }
    }

    pub async fn list_pull_requests(
        &self,
        project: &str,
        repo_id: &str,
    ) -> Result<Vec<PullRequest>, AppError> {
        match self {
            GitClient::Ado(c) => c.list_pull_requests(project, repo_id).await,
            GitClient::Github(c) => c.list_pull_requests(project, repo_id).await,
        }
    }

    pub async fn get_pull_request(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
    ) -> Result<PullRequest, AppError> {
        match self {
            GitClient::Ado(c) => c.get_pull_request(project, repo_id, pr_id).await,
            GitClient::Github(c) => c.get_pull_request(project, repo_id, pr_id).await,
        }
    }

    /// PR status checks. ADO ignores `repo_id` (its policy API keys off
    /// project + PR); GitHub needs it to locate the head commit's check-runs.
    pub async fn list_pr_policy_evaluations(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
    ) -> Result<Vec<PrCheck>, AppError> {
        match self {
            GitClient::Ado(c) => c.list_pr_policy_evaluations(project, pr_id).await,
            GitClient::Github(c) => c.list_pr_policy_evaluations(project, repo_id, pr_id).await,
        }
    }

    pub async fn get_iterations(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
    ) -> Result<Vec<Iteration>, AppError> {
        match self {
            GitClient::Ado(c) => c.get_iterations(project, repo_id, pr_id).await,
            GitClient::Github(c) => c.get_iterations(project, repo_id, pr_id).await,
        }
    }

    pub async fn changed_paths_since_iteration(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        from_iteration: i32,
        to_iteration: i32,
    ) -> Result<HashSet<String>, AppError> {
        match self {
            GitClient::Ado(c) => {
                c.changed_paths_since_iteration(project, repo_id, pr_id, from_iteration, to_iteration)
                    .await
            }
            GitClient::Github(c) => {
                c.changed_paths_since_iteration(project, repo_id, pr_id, from_iteration, to_iteration)
                    .await
            }
        }
    }

    pub async fn get_pr_files(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        iteration: i32,
    ) -> Result<PrFilesResult, AppError> {
        match self {
            GitClient::Ado(c) => c.get_pr_files(project, repo_id, pr_id, iteration).await,
            GitClient::Github(c) => c.get_pr_files(project, repo_id, pr_id, iteration).await,
        }
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
        match self {
            GitClient::Ado(c) => {
                c.get_file_diff(project, repo_id, pr_id, file_path, iteration, view)
                    .await
            }
            GitClient::Github(c) => {
                c.get_file_diff(project, repo_id, pr_id, file_path, iteration, view)
                    .await
            }
        }
    }

    pub async fn get_file_lines(
        &self,
        project: &str,
        repo_id: &str,
        commit_id: &str,
        file_path: &str,
        start_line: usize,
        end_line: usize,
    ) -> Result<Vec<String>, AppError> {
        match self {
            GitClient::Ado(c) => {
                c.get_file_lines(project, repo_id, commit_id, file_path, start_line, end_line)
                    .await
            }
            GitClient::Github(c) => {
                c.get_file_lines(project, repo_id, commit_id, file_path, start_line, end_line)
                    .await
            }
        }
    }

    pub async fn get_file_at_commit(
        &self,
        project: &str,
        repo_id: &str,
        commit_id: &str,
        file_path: &str,
    ) -> Result<Option<String>, AppError> {
        match self {
            GitClient::Ado(c) => {
                c.get_file_at_commit(project, repo_id, commit_id, file_path)
                    .await
            }
            GitClient::Github(c) => {
                c.get_file_at_commit(project, repo_id, commit_id, file_path)
                    .await
            }
        }
    }

    pub async fn get_threads(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
    ) -> Result<Vec<CommentThread>, AppError> {
        match self {
            GitClient::Ado(c) => c.get_threads(project, repo_id, pr_id).await,
            GitClient::Github(c) => c.get_threads(project, repo_id, pr_id).await,
        }
    }

    pub async fn post_thread(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        thread: &serde_json::Value,
    ) -> Result<CommentThread, AppError> {
        match self {
            GitClient::Ado(c) => c.post_thread(project, repo_id, pr_id, thread).await,
            GitClient::Github(c) => c.post_thread(project, repo_id, pr_id, thread).await,
        }
    }

    pub async fn add_comment_to_thread(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        thread_id: i64,
        comment: &serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        match self {
            GitClient::Ado(c) => {
                c.add_comment_to_thread(project, repo_id, pr_id, thread_id, comment)
                    .await
            }
            GitClient::Github(c) => {
                c.add_comment_to_thread(project, repo_id, pr_id, thread_id, comment)
                    .await
            }
        }
    }

    pub async fn update_comment(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        thread_id: i64,
        comment_id: i64,
        content: &str,
        is_pr_level: bool,
    ) -> Result<serde_json::Value, AppError> {
        match self {
            GitClient::Ado(c) => {
                c.update_comment(project, repo_id, pr_id, thread_id, comment_id, content)
                    .await
            }
            GitClient::Github(c) => {
                c.update_comment(project, repo_id, pr_id, comment_id, content, is_pr_level)
                    .await
            }
        }
    }

    pub async fn update_reviewer_status(
        &self,
        project: &str,
        repo_id: &str,
        pr_id: i64,
        reviewer_id: &str,
        vote: i32,
    ) -> Result<(), AppError> {
        match self {
            GitClient::Ado(c) => {
                c.update_reviewer_status(project, repo_id, pr_id, reviewer_id, vote)
                    .await
            }
            GitClient::Github(c) => {
                c.update_reviewer_status(project, repo_id, pr_id, reviewer_id, vote)
                    .await
            }
        }
    }

    pub async fn get_authenticated_user_id(&self) -> Result<String, AppError> {
        match self {
            GitClient::Ado(c) => c.get_authenticated_user_id().await,
            GitClient::Github(c) => c.get_authenticated_user_id().await,
        }
    }
}
