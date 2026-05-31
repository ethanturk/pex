//! Provider-neutral data structures returned by every Git provider client.
//!
//! These types were originally defined inline in `ado/mod.rs` and carry serde
//! `rename` attributes matching Azure DevOps' JSON shape so ADO responses
//! deserialize straight into them. Other providers (e.g. GitHub) construct the
//! same structs by hand from their own API responses, so the renames are inert
//! there — they only affect ADO deserialization.

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
pub struct PrCheck {
    pub id: String,
    pub name: String,
    pub status: String,
    #[serde(rename = "isRequired")]
    pub is_required: bool,
    pub description: String,
    #[serde(rename = "startedDate")]
    pub started_date: Option<String>,
    #[serde(rename = "completedDate")]
    pub completed_date: Option<String>,
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
    // ADO omits `name` on some iterations (notably auto-generated "rebase"
    // or merge-target-update iterations), so this must be optional or the
    // entire iterations response fails to deserialize.
    #[serde(default)]
    pub name: Option<String>,
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
    #[serde(rename = "isDeleted", default)]
    pub is_deleted: bool,
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
    #[serde(rename = "isDeleted", default)]
    pub is_deleted: bool,
}
