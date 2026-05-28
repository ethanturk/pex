//! Resolve project conventions and style guides for a changed file so the
//! Review prompt can ground its feedback in the repo's own standards.
//!
//! Walks the parent directories of the changed file from leaf → root, trying
//! a small set of case variants for each filename family, and returns the
//! nearest hit for AGENTS and STYLE independently. Results (including misses)
//! are memoized in `StandardsCache` so a full "Review All" run over a 40-file
//! PR makes only a handful of ADO requests instead of hundreds.

use crate::ado::AdoClient;
use crate::cache::standards_cache::{StandardsCache, StandardsCacheKey};

/// Filename variants for the agent-guidance file. Tried in this order; first
/// hit wins. ADO paths are case-sensitive, so we have to enumerate.
const AGENTS_VARIANTS: &[&str] = &["AGENTS.md", "agents.md", "Agents.md"];
const STYLE_VARIANTS: &[&str] = &["STYLE.md", "style.md", "Style.md"];

pub struct ResolvedDoc {
    /// Path in the repo where the file was found (e.g. `Agentic-AI/AGENTS.md`).
    pub path: String,
    /// Text injected into the prompt — already truncated and (if clipped) annotated.
    pub content: String,
}

#[derive(Default)]
pub struct StandardsContext {
    pub agents: Option<ResolvedDoc>,
    pub style: Option<ResolvedDoc>,
}

impl StandardsContext {
    pub fn is_empty(&self) -> bool {
        self.agents.is_none() && self.style.is_none()
    }
}

/// Walk leaf → root from `file_path`, resolving both AGENTS and STYLE.
/// `max_chars` is applied per-file with an explicit truncation marker.
pub async fn resolve(
    client: &AdoClient,
    cache: &StandardsCache,
    org_url: &str,
    project_id: &str,
    repo_id: &str,
    commit: &str,
    file_path: &str,
    max_chars: usize,
) -> StandardsContext {
    let dirs = walk_dirs(file_path);

    let agents = find_nearest(
        client,
        cache,
        org_url,
        project_id,
        repo_id,
        commit,
        &dirs,
        AGENTS_VARIANTS,
        max_chars,
    )
    .await;
    let style = find_nearest(
        client,
        cache,
        org_url,
        project_id,
        repo_id,
        commit,
        &dirs,
        STYLE_VARIANTS,
        max_chars,
    )
    .await;

    StandardsContext { agents, style }
}

/// Produce the list of directories to probe, starting at the file's own
/// directory and ending at the repo root (empty string). Always at least one
/// entry (the root) so a file with no parent dirs still gets a root probe.
fn walk_dirs(file_path: &str) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    let mut current = std::path::Path::new(file_path).parent();
    while let Some(p) = current {
        let s = p.to_string_lossy().to_string();
        dirs.push(s.clone());
        if s.is_empty() {
            break;
        }
        current = p.parent();
    }
    // Always probe the repo root last in case the file path has no separators
    // (e.g., a top-level file like `README.md`).
    if !dirs.iter().any(|d| d.is_empty()) {
        dirs.push(String::new());
    }
    dirs
}

async fn find_nearest(
    client: &AdoClient,
    cache: &StandardsCache,
    org_url: &str,
    project_id: &str,
    repo_id: &str,
    commit: &str,
    dirs: &[String],
    variants: &[&str],
    max_chars: usize,
) -> Option<ResolvedDoc> {
    for dir in dirs {
        for variant in variants {
            let path = if dir.is_empty() {
                (*variant).to_string()
            } else {
                format!("{}/{}", dir, variant)
            };
            let key = StandardsCacheKey {
                org_url: org_url.to_string(),
                project_id: project_id.to_string(),
                repo_id: repo_id.to_string(),
                commit: commit.to_string(),
                path: path.clone(),
            };

            let cached = cache.get(&key);
            let raw = match cached {
                Some(v) => v,
                None => {
                    // Fetch & memoize. ADO errors mean "we don't know" — cache
                    // nothing so a transient failure doesn't lock in a miss.
                    match client
                        .get_file_at_commit(project_id, repo_id, commit, &path)
                        .await
                    {
                        Ok(v) => {
                            cache.put(key, v.clone());
                            v
                        }
                        Err(_) => continue,
                    }
                }
            };

            if let Some(content) = raw {
                if content.trim().is_empty() {
                    continue;
                }
                let truncated = truncate(&content, max_chars);
                return Some(ResolvedDoc {
                    path,
                    content: truncated,
                });
            }
        }
    }
    None
}

/// Truncate a UTF-8 string to roughly `max_chars` characters and append a
/// visible marker if anything was clipped. Boundary-safe on multi-byte chars.
fn truncate(s: &str, max_chars: usize) -> String {
    if max_chars == 0 || s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    let remaining = s.chars().count() - max_chars;
    out.push_str(&format!(
        "\n\n[truncated — {} more characters omitted]",
        remaining
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_dirs_monorepo_path() {
        let dirs = walk_dirs("Agentic-AI/Action-Agent/src/foo.py");
        assert_eq!(
            dirs,
            vec![
                "Agentic-AI/Action-Agent/src".to_string(),
                "Agentic-AI/Action-Agent".to_string(),
                "Agentic-AI".to_string(),
                "".to_string(),
            ]
        );
    }

    #[test]
    fn walk_dirs_root_file() {
        let dirs = walk_dirs("README.md");
        assert_eq!(dirs, vec!["".to_string()]);
    }

    #[test]
    fn truncate_keeps_short_strings() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn truncate_annotates_clipped_strings() {
        let out = truncate("abcdefghij", 5);
        assert!(out.starts_with("abcde"));
        assert!(out.contains("5 more characters omitted"));
    }
}
