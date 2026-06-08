//! Headless PR review for CI (Phase 4).
//!
//! Reviews an Azure DevOps pull request server-side — no desktop app — using the
//! same engine the app does, and exits non-zero if it finds a Blocking issue, so
//! it can gate a pipeline. This is where "scaling" lands: reviews run wherever CI
//! runs, not only on a reviewer's machine.
//!
//! Usage:
//!   cd src-tauri
//!   PEX_ADO_PAT=...                         # Azure DevOps PAT (Code: Read)
//!   PEX_AI_PROVIDER=openai \                # or anthropic
//!   PEX_AI_ENDPOINT=https://api.openai.com \
//!   PEX_AI_MODEL=gpt-4.1 \
//!   PEX_AI_KEY=sk-... \
//!   cargo run --example review_cli -- "https://dev.azure.com/<org>/<project>/_git/<repo>/pullrequest/<id>"
//!
//! Or pass coordinates explicitly via env instead of a URL:
//!   PEX_ORG_URL, PEX_ADO_PROJECT, PEX_ADO_REPO, PEX_ADO_PR
//!
//! Tuning (optional): PEX_REVIEW_MODE=fast|thorough (default fast),
//!   PEX_CONFIDENCE_THRESHOLD (default 80), PEX_BLOCKING_CONFIDENCE (default 85).
//!
//! Exit codes: 0 = no blocking findings · 1 = blocking findings · 2 = setup error.

use std::sync::Arc;

use pex_lib::ado::AdoClient;
use pex_lib::ai::anthropic::AnthropicProvider;
use pex_lib::ai::openai::OpenAiProvider;
use pex_lib::ai::AiProvider;
use pex_lib::diff::engine::DiffView;
use pex_lib::review::engine::{review_single_file, tier_for, FileInput, Tier};
use pex_lib::review::state::ReviewMode;

struct Coords {
    org_url: String,
    project: String,
    repo: String,
    pr_id: i64,
}

/// Parse `https://dev.azure.com/{org}/{project}/_git/{repo}/pullrequest/{id}`
/// (or a `*.visualstudio.com` host) into review coordinates.
fn parse_pr_url(url: &str) -> Option<Coords> {
    let parsed = url::Url::parse(url.trim().trim_end_matches('/')).ok()?;
    let host = parsed.host_str()?.to_string();
    let scheme = parsed.scheme();
    let segs: Vec<&str> = parsed.path_segments()?.collect();
    let git_idx = segs.iter().position(|s| *s == "_git")?;
    if git_idx == 0 || segs.len() < git_idx + 2 {
        return None;
    }
    let pr_id = segs.last()?.parse::<i64>().ok()?;
    let repo = decode(segs.get(git_idx + 1)?);
    let project = decode(segs.get(git_idx - 1)?);
    let org_path = &segs[..git_idx - 1];
    let org_url = if org_path.is_empty() {
        format!("{scheme}://{host}")
    } else {
        format!("{scheme}://{host}/{}", org_path.join("/"))
    };
    Some(Coords {
        org_url,
        project,
        repo,
        pr_id,
    })
}

fn decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={s}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| s.to_string())
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

fn coords_from_args_or_env() -> Option<Coords> {
    if let Some(url) = std::env::args().nth(1) {
        if let Some(c) = parse_pr_url(&url) {
            return Some(c);
        }
        eprintln!("Could not parse PR URL: {url}");
    }
    Some(Coords {
        org_url: env("PEX_ORG_URL")?,
        project: env("PEX_ADO_PROJECT")?,
        repo: env("PEX_ADO_REPO")?,
        pr_id: env("PEX_ADO_PR")?.parse().ok()?,
    })
}

fn build_provider() -> Arc<dyn AiProvider> {
    let kind = env("PEX_AI_PROVIDER").unwrap_or_else(|| "openai".into());
    let endpoint = env("PEX_AI_ENDPOINT").unwrap_or_else(|| match kind.as_str() {
        "anthropic" => "https://api.anthropic.com".into(),
        _ => "https://api.openai.com".into(),
    });
    let model = env("PEX_AI_MODEL").unwrap_or_else(|| "gpt-4.1".into());
    let Some(key) = env("PEX_AI_KEY") else {
        eprintln!("PEX_AI_KEY is required.");
        std::process::exit(2);
    };
    match kind.as_str() {
        "anthropic" => Arc::new(AnthropicProvider::new(endpoint, model, key, 10, 120)),
        _ => Arc::new(OpenAiProvider::new(endpoint, model, key, 10, 120)),
    }
}

#[tokio::main]
async fn main() {
    let Some(coords) = coords_from_args_or_env() else {
        eprintln!(
            "Provide a PR URL argument, or set PEX_ORG_URL / PEX_ADO_PROJECT / PEX_ADO_REPO / PEX_ADO_PR."
        );
        std::process::exit(2);
    };
    let Some(pat) = env("PEX_ADO_PAT") else {
        eprintln!("PEX_ADO_PAT is required (Azure DevOps personal access token).");
        std::process::exit(2);
    };

    let mode = match env("PEX_REVIEW_MODE").as_deref() {
        Some("thorough") => ReviewMode::Thorough,
        _ => ReviewMode::Fast,
    };
    let threshold: u8 = env("PEX_CONFIDENCE_THRESHOLD")
        .and_then(|s| s.parse().ok())
        .unwrap_or(pex_lib::ai::DEFAULT_CONFIDENCE_THRESHOLD);
    let blocking: u8 = env("PEX_BLOCKING_CONFIDENCE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(pex_lib::ai::DEFAULT_BLOCKING_CONFIDENCE);

    let client = AdoClient::new(coords.org_url.clone(), pat);
    let provider = build_provider();

    // Latest iteration = highest iteration id.
    let iteration = client
        .get_iterations(&coords.project, &coords.repo, coords.pr_id)
        .await
        .ok()
        .and_then(|its| its.into_iter().map(|i| i.id).max())
        .and_then(|id| i32::try_from(id).ok())
        .unwrap_or(1);

    let files = match client
        .get_pr_files(&coords.project, &coords.repo, coords.pr_id, iteration)
        .await
    {
        Ok(r) => r.files,
        Err(e) => {
            eprintln!("Failed to list PR files: {e}");
            std::process::exit(2);
        }
    };

    eprintln!(
        "Reviewing PR #{} ({} file(s), iteration {}, mode {:?})\n",
        coords.pr_id,
        files.len(),
        iteration,
        mode
    );

    // (file_path, severity, confidence, line, comment, tier)
    let mut blocking_count = 0usize;
    let mut should_fix = 0usize;
    let mut nit = 0usize;
    let mut fyi = 0usize;
    let mut printed_any = false;

    for change in files {
        let path = change.item.path.trim_start_matches('/').to_string();
        let diff = match client
            .get_file_diff(
                &coords.project,
                &coords.repo,
                coords.pr_id,
                &path,
                iteration,
                DiffView::Inline,
            )
            .await
        {
            Ok(d) => d,
            Err(_) => continue, // binary/unavailable file — skip
        };

        let input = FileInput {
            path: path.clone(),
            old_content: diff.old_content,
            new_content: diff.new_content,
        };
        let result =
            match review_single_file(provider.clone(), mode, &input, "", threshold, 1).await {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("  [{path}] review error: {e}");
                    continue;
                }
            };

        for f in &result.findings {
            let tier = tier_for(f.severity, f.confidence, f.line_start, blocking);
            match tier {
                Tier::Blocking => blocking_count += 1,
                Tier::ShouldFix => should_fix += 1,
                Tier::Nit => nit += 1,
                Tier::Fyi => fyi += 1,
            }
            let loc = match f.line_start {
                Some(l) => format!("{path}:{l}"),
                None => path.clone(),
            };
            println!(
                "{} {} ({}%) — {}",
                tier_tag(tier),
                loc,
                f.confidence,
                f.comment.trim()
            );
            printed_any = true;
        }
    }

    if !printed_any {
        println!("No findings above the confidence threshold.");
    }
    eprintln!(
        "\nSummary: {blocking_count} blocking, {should_fix} should-fix, {nit} nit, {fyi} FYI"
    );

    if blocking_count > 0 {
        eprintln!("FAIL: {blocking_count} blocking finding(s).");
        std::process::exit(1);
    }
    eprintln!("PASS: no blocking findings.");
}

fn tier_tag(t: Tier) -> &'static str {
    match t {
        Tier::Blocking => "[BLOCKING]",
        Tier::ShouldFix => "[SHOULD-FIX]",
        Tier::Nit => "[NIT]",
        Tier::Fyi => "[FYI]",
    }
}
