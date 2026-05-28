//! ADO items-endpoint probe — minimal-friction version.
//!
//! Usage:
//!   cd src-tauri
//!   cargo run --example probe_diff -- "<PR URL>"
//!
//!   # e.g.
//!   cargo run --example probe_diff -- "https://dev.azure.com/inEight/Agentic-AI/_git/MyRepo/pullrequest/1234"
//!
//! The probe reuses the PAT the running Pex app already stored in your OS
//! keyring (no need to paste it). It auto-picks the first blob file in the
//! PR's first iteration and tries 6 ADO URL variants on both base + source
//! commits, then prints a verdict for the most reliable variant.
//!
//! Optional overrides:
//!   PEX_PAT="..."          PAT to use instead of keyring lookup
//!   PEX_FILE_PATH="..."    Probe this specific path instead of the auto-picked one
//!   PEX_ITERATION="2"      Probe a non-default iteration (default 1)

use std::env;

use base64::Engine;
use keyring::Entry;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use rusqlite::Connection;
use serde_json::Value;

const API_VERSION: &str = "7.0";
const KEYRING_SERVICE: &str = "pex-pr-reviewer";

/// Open the same SQLite DB the running app uses and list (orgUrl, name) rows.
/// We use this to discover the *exact* URL spelling the PAT was saved under,
/// since `dev.azure.com/inEight` and `dev.azure.com/ineight` are distinct
/// keyring entries.
fn list_saved_orgs() -> Vec<String> {
    let data_dir = if let Ok(d) = std::env::var("XDG_DATA_HOME") {
        format!("{d}/pex")
    } else if let Ok(home) = std::env::var("HOME") {
        if cfg!(target_os = "macos") {
            format!("{home}/Library/Application Support/com.pex.pr-reviewer")
        } else if cfg!(target_os = "linux") {
            format!("{home}/.local/share/pex")
        } else {
            return Vec::new();
        }
    } else {
        return Vec::new();
    };
    let db_path = format!("{data_dir}/pex.db");
    let Ok(conn) = Connection::open(&db_path) else {
        return Vec::new();
    };
    let mut stmt = match conn.prepare("SELECT org_url FROM orgs") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| r.get::<_, String>(0));
    match rows {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Pick whichever saved org URL matches the parsed PR URL once both are
/// normalized (lowercased, trailing-slash-stripped). Returns the *original*
/// stored spelling so the keyring lookup hits the right entry.
fn resolve_org_url(parsed: &str) -> Option<String> {
    let norm = |s: &str| s.trim_end_matches('/').to_ascii_lowercase();
    let target = norm(parsed);
    list_saved_orgs().into_iter().find(|o| norm(o) == target)
}

struct PrCoords {
    org_url: String, // e.g. https://dev.azure.com/inEight
    project: String,
    repo: String,
    pr_id: String,
}

/// Parse a PR URL like:
///   https://dev.azure.com/{org}/{project}/_git/{repo}/pullrequest/{id}
///   https://{org}.visualstudio.com/{project}/_git/{repo}/pullrequest/{id}
fn parse_pr_url(url: &str) -> Option<PrCoords> {
    let trimmed = url.trim().trim_end_matches('/');
    let parsed = url::Url::parse(trimmed).ok()?;
    let host = parsed.host_str()?.to_string();
    let scheme = parsed.scheme().to_string();
    let segs: Vec<&str> = parsed.path_segments()?.collect();

    // pullrequest/{id} must be the last two segments.
    if segs.len() < 5 {
        return None;
    }
    let last = segs.last()?;
    let pr_id = last.parse::<u64>().ok()?.to_string();
    let prev = segs.get(segs.len() - 2)?;
    if !prev.eq_ignore_ascii_case("pullrequest") && !prev.eq_ignore_ascii_case("pullRequests") {
        return None;
    }

    // Walk back to find "_git/{repo}".
    let git_idx = segs.iter().position(|s| *s == "_git")?;
    if git_idx == 0 {
        return None;
    }
    let repo = segs.get(git_idx + 1)?.to_string();
    let project = segs.get(git_idx - 1)?.to_string();

    // org_url is everything before the project segment.
    let org_segs = &segs[..git_idx - 1];
    let org_path = if org_segs.is_empty() {
        String::new()
    } else {
        format!("/{}", org_segs.join("/"))
    };
    let org_url = format!("{scheme}://{host}{org_path}");

    Some(PrCoords {
        org_url,
        project: urlencoding_decode(&project),
        repo: urlencoding_decode(&repo),
        pr_id,
    })
}

fn urlencoding_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", s).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_else(|| s.to_string())
}

fn pat_from_keyring(org_url: &str) -> Option<String> {
    let entry = Entry::new(KEYRING_SERVICE, &format!("pat:{}", org_url)).ok()?;
    entry.get_password().ok()
}

/// Look up the PAT for the parsed org URL, trying:
///   1. The URL spelling recorded in the app's SQLite (exact match it was
///      saved under — handles case differences like `inEight` vs `ineight`).
///   2. Bare variants: as-given, with/without trailing slash, lowercased host.
/// Returns (pat, url_used).
fn find_pat(parsed_org_url: &str) -> Option<(String, String)> {
    if let Some(saved) = resolve_org_url(parsed_org_url) {
        if let Some(p) = pat_from_keyring(&saved) {
            return Some((p, saved));
        }
    }
    let mut candidates: Vec<String> = vec![
        parsed_org_url.to_string(),
        parsed_org_url.trim_end_matches('/').to_string(),
        format!("{}/", parsed_org_url.trim_end_matches('/')),
        parsed_org_url.to_ascii_lowercase(),
    ];
    candidates.dedup();
    for c in candidates {
        if let Some(p) = pat_from_keyring(&c) {
            return Some((p, c));
        }
    }
    None
}

fn auth_headers(pat: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    let token = base64::engine::general_purpose::STANDARD.encode(format!(":{pat}"));
    h.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Basic {token}")).unwrap(),
    );
    h
}

fn enc_keep_slash(s: &str) -> String {
    s.split('/')
        .map(|seg| {
            seg.replace('%', "%25")
                .replace(' ', "%20")
                .replace('#', "%23")
                .replace('&', "%26")
                .replace('?', "%3F")
                .replace('+', "%2B")
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn enc_full(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('?', "%3F")
        .replace('+', "%2B")
        .replace('/', "%2F")
}

async fn first_changed_file(
    client: &reqwest::Client,
    headers: &HeaderMap,
    org: &str,
    project: &str,
    repo: &str,
    pr_id: &str,
    iteration: &str,
) -> Option<String> {
    let url = format!(
        "{org}/{project}/_apis/git/repositories/{repo}/pullRequests/{pr_id}/iterations/{iteration}/changes?$top=20&api-version={API_VERSION}"
    );
    eprintln!("\n[probe] fetch iteration changes\n  {url}");
    let resp = client
        .get(&url)
        .headers(headers.clone())
        .send()
        .await
        .ok()?;
    let status = resp.status();
    let body = resp.text().await.ok()?;
    eprintln!("  status: {status}");
    if !status.is_success() {
        eprintln!("  body: {}", body.chars().take(400).collect::<String>());
        return None;
    }
    let v: Value = serde_json::from_str(&body).ok()?;
    let entries = v.get("changeEntries")?.as_array()?;
    for e in entries {
        let item = e.get("item")?;
        let path = item.get("path")?.as_str()?;
        let typ = item
            .get("gitObjectType")
            .and_then(|t| t.as_str())
            .unwrap_or("blob");
        if typ.eq_ignore_ascii_case("blob") {
            eprintln!("  picked: {path}");
            return Some(path.to_string());
        }
    }
    None
}

async fn iteration_commits(
    client: &reqwest::Client,
    headers: &HeaderMap,
    org: &str,
    project: &str,
    repo: &str,
    pr_id: &str,
    iteration: &str,
) -> Option<(String, Option<String>)> {
    let url = format!(
        "{org}/{project}/_apis/git/repositories/{repo}/pullRequests/{pr_id}/iterations/{iteration}?api-version={API_VERSION}"
    );
    eprintln!("\n[probe] fetch iteration detail\n  {url}");
    let resp = client
        .get(&url)
        .headers(headers.clone())
        .send()
        .await
        .ok()?;
    let status = resp.status();
    let body = resp.text().await.ok()?;
    eprintln!("  status: {status}");
    if !status.is_success() {
        eprintln!("  body: {}", body.chars().take(400).collect::<String>());
        return None;
    }
    let v: Value = serde_json::from_str(&body).ok()?;
    let source = v
        .get("sourceRefCommit")?
        .get("commitId")?
        .as_str()?
        .to_string();
    let base = v
        .get("commonRefCommit")
        .and_then(|c| c.get("commitId"))
        .and_then(|c| c.as_str())
        .map(String::from);
    eprintln!("  source: {source}");
    eprintln!("  base:   {}", base.as_deref().unwrap_or("<none>"));
    Some((source, base))
}

struct Variant {
    label: &'static str,
    build_url: fn(&str, &str, &str, &str, &str) -> String,
    accept: &'static str,
}

fn variants() -> Vec<Variant> {
    vec![
        Variant {
            label: "A: rooted path, $format=text, download=true, Accept */*",
            build_url: |org, project, repo, path, commit| {
                let p = if path.starts_with('/') {
                    path.to_string()
                } else {
                    format!("/{path}")
                };
                let pe = enc_keep_slash(&p);
                format!("{org}/{project}/_apis/git/repositories/{repo}/items?path={pe}&versionDescriptor.version={commit}&versionDescriptor.versionType=commit&download=true&$format=text&api-version={API_VERSION}")
            },
            accept: "*/*",
        },
        Variant {
            label: "B: rooted path, $format=text, Accept text/plain",
            build_url: |org, project, repo, path, commit| {
                let p = if path.starts_with('/') {
                    path.to_string()
                } else {
                    format!("/{path}")
                };
                let pe = enc_keep_slash(&p);
                format!("{org}/{project}/_apis/git/repositories/{repo}/items?path={pe}&versionDescriptor.version={commit}&versionDescriptor.versionType=commit&$format=text&api-version={API_VERSION}")
            },
            accept: "text/plain",
        },
        Variant {
            label: "C: rooted path, includeContent=true, Accept application/json",
            build_url: |org, project, repo, path, commit| {
                let p = if path.starts_with('/') {
                    path.to_string()
                } else {
                    format!("/{path}")
                };
                let pe = enc_keep_slash(&p);
                format!("{org}/{project}/_apis/git/repositories/{repo}/items?path={pe}&versionDescriptor.version={commit}&versionDescriptor.versionType=commit&includeContent=true&api-version={API_VERSION}")
            },
            accept: "application/json",
        },
        Variant {
            label: "D: rootless path, $format=text, Accept */*",
            build_url: |org, project, repo, path, commit| {
                let p = path.strip_prefix('/').unwrap_or(path);
                let pe = enc_keep_slash(p);
                format!("{org}/{project}/_apis/git/repositories/{repo}/items?path={pe}&versionDescriptor.version={commit}&versionDescriptor.versionType=commit&$format=text&api-version={API_VERSION}")
            },
            accept: "*/*",
        },
        Variant {
            label: "E: %2F-encoded slashes, $format=text",
            build_url: |org, project, repo, path, commit| {
                let p = if path.starts_with('/') {
                    path.to_string()
                } else {
                    format!("/{path}")
                };
                let pe = enc_full(&p);
                format!("{org}/{project}/_apis/git/repositories/{repo}/items?path={pe}&versionDescriptor.version={commit}&versionDescriptor.versionType=commit&$format=text&api-version={API_VERSION}")
            },
            accept: "*/*",
        },
        Variant {
            label: "F: scopePath form (path-as-scope, no $format)",
            build_url: |org, project, repo, path, commit| {
                let p = if path.starts_with('/') {
                    path.to_string()
                } else {
                    format!("/{path}")
                };
                let pe = enc_keep_slash(&p);
                format!("{org}/{project}/_apis/git/repositories/{repo}/items?scopePath={pe}&recursionLevel=none&versionDescriptor.version={commit}&versionDescriptor.versionType=commit&includeContent=true&api-version={API_VERSION}")
            },
            accept: "application/json",
        },
    ]
}

enum Verdict {
    RealText { bytes: usize },
    JsonWithContent { bytes: usize },
    JsonNoContent,
    Empty,
    Error(String),
}

fn summarize(status: reqwest::StatusCode, content_type: &str, body: &str) -> (String, Verdict) {
    if !status.is_success() {
        return (
            format!(
                "HTTP {status}; first 200 chars: {}",
                body.chars().take(200).collect::<String>()
            ),
            Verdict::Error(format!("HTTP {status}")),
        );
    }
    let trimmed = body.trim_start();
    if content_type.contains("application/json") || trimmed.starts_with('{') {
        match serde_json::from_str::<Value>(body) {
            Ok(v) => {
                let content_state = match v.get("content") {
                    Some(Value::String(s)) if !s.is_empty() => {
                        let preview: String =
                            s.chars().take(120).collect::<String>().replace('\n', "\\n");
                        return (
                            format!(
                                "JSON envelope; content present ({} bytes); preview: {preview}",
                                s.len()
                            ),
                            Verdict::JsonWithContent { bytes: s.len() },
                        );
                    }
                    Some(Value::String(_)) => "empty string".to_string(),
                    Some(Value::Null) => "null".to_string(),
                    Some(other) => format!("unexpected: {other}"),
                    None => "missing".to_string(),
                };
                (
                    format!(
                        "JSON envelope; content={content_state}; full preview: {}",
                        body.chars().take(200).collect::<String>()
                    ),
                    Verdict::JsonNoContent,
                )
            }
            Err(e) => (
                format!(
                    "looked like JSON but failed to parse ({e}); first 200: {}",
                    body.chars().take(200).collect::<String>()
                ),
                Verdict::Error("invalid JSON".into()),
            ),
        }
    } else if body.is_empty() {
        ("empty body".to_string(), Verdict::Empty)
    } else {
        let preview: String = body
            .chars()
            .take(160)
            .collect::<String>()
            .replace('\n', "\\n");
        (
            format!("raw text, {} bytes; first 160: {preview}", body.len()),
            Verdict::RealText { bytes: body.len() },
        )
    }
}

fn verdict_score(v: &Verdict) -> i32 {
    match v {
        Verdict::RealText { .. } => 100,
        Verdict::JsonWithContent { .. } => 90,
        Verdict::JsonNoContent => 10,
        Verdict::Empty => 5,
        Verdict::Error(_) => 0,
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let pr_url = args.get(1).cloned().or_else(|| env::var("PEX_PR_URL").ok());
    let pr_url = match pr_url {
        Some(u) if !u.is_empty() => u,
        _ => {
            eprintln!("Usage: cargo run --example probe_diff -- <PR URL>");
            eprintln!("Or:    PEX_PR_URL=<URL> cargo run --example probe_diff");
            std::process::exit(2);
        }
    };

    let coords = match parse_pr_url(&pr_url) {
        Some(c) => c,
        None => {
            eprintln!(
                "Couldn't parse PR URL: {pr_url}\nExpected shape: https://dev.azure.com/<org>/<project>/_git/<repo>/pullrequest/<id>"
            );
            std::process::exit(2);
        }
    };

    eprintln!(
        "[probe] org={}\n[probe] project={}\n[probe] repo={}\n[probe] pr_id={}",
        coords.org_url, coords.project, coords.repo, coords.pr_id
    );

    let (pat, org_url_for_api) = if let Ok(p) = env::var("PEX_PAT") {
        eprintln!("[probe] using PAT from PEX_PAT env var");
        (p, coords.org_url.clone())
    } else if let Some((p, used)) = find_pat(&coords.org_url) {
        eprintln!("[probe] using PAT from OS keyring (entry pat:{used})");
        // Use the saved URL for API calls too — keyring URL and ADO URL must
        // match exactly (e.g. casing) for the org's actual host.
        (p, used)
    } else {
        let saved = list_saved_orgs();
        eprintln!(
            "No PAT in keyring for {} and PEX_PAT not set.",
            coords.org_url
        );
        if !saved.is_empty() {
            eprintln!("Orgs the running app has saved:");
            for o in saved {
                eprintln!("  {o}");
            }
            eprintln!("If the PR URL spelling differs from one of these (e.g. casing), pass that exact URL instead.");
        } else {
            eprintln!(
                "(No saved orgs found in {KEYRING_SERVICE} / pex.db — sign in to Pex first.)"
            );
        }
        std::process::exit(1);
    };

    // Use the resolved org URL (with correct spelling) for the rest of the run.
    let coords = PrCoords {
        org_url: org_url_for_api,
        ..coords
    };

    let iteration = env::var("PEX_ITERATION").unwrap_or_else(|_| "1".to_string());
    let client = reqwest::Client::new();
    let headers = auth_headers(&pat);

    let path = match env::var("PEX_FILE_PATH").ok().filter(|s| !s.is_empty()) {
        Some(p) => p,
        None => match first_changed_file(
            &client,
            &headers,
            &coords.org_url,
            &coords.project,
            &coords.repo,
            &coords.pr_id,
            &iteration,
        )
        .await
        {
            Some(p) => p,
            None => {
                eprintln!("Could not pick a file to probe. Try PEX_FILE_PATH=... or different PEX_ITERATION.");
                std::process::exit(1);
            }
        },
    };

    let (source_commit, base_commit) = match iteration_commits(
        &client,
        &headers,
        &coords.org_url,
        &coords.project,
        &coords.repo,
        &coords.pr_id,
        &iteration,
    )
    .await
    {
        Some(t) => t,
        None => {
            eprintln!("Could not fetch iteration detail.");
            std::process::exit(1);
        }
    };

    let mut sides: Vec<(&str, String)> = vec![("source", source_commit.clone())];
    if let Some(b) = base_commit.clone() {
        sides.push(("base", b));
    }

    let mut best_score = -1;
    let mut best_label = String::new();

    for (side_name, commit) in sides {
        println!("\n========== {side_name} side @ {commit} ==========");
        println!("path = {path}\n");
        for v in variants() {
            let url = (v.build_url)(
                &coords.org_url,
                &coords.project,
                &coords.repo,
                &path,
                &commit,
            );
            let mut h = headers.clone();
            h.insert(
                ACCEPT,
                HeaderValue::from_str(v.accept).unwrap_or(HeaderValue::from_static("*/*")),
            );

            print!("[{}] ", v.label);
            let resp = match client.get(&url).headers(h).send().await {
                Ok(r) => r,
                Err(e) => {
                    println!("TRANSPORT ERROR: {e}");
                    continue;
                }
            };
            let status = resp.status();
            let content_type = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("?")
                .to_string();
            let body = resp.text().await.unwrap_or_default();
            let (summary, verdict) = summarize(status, &content_type, &body);
            println!("status={status} ct={content_type}\n  → {summary}");

            let score = verdict_score(&verdict);
            if score > best_score {
                best_score = score;
                best_label = v.label.to_string();
            }
            // Dump full URL only when nothing worked, to keep output tight.
            if matches!(
                verdict,
                Verdict::Error(_) | Verdict::Empty | Verdict::JsonNoContent
            ) {
                println!("  url: {url}");
            }
        }
    }

    println!("\n==================================================");
    if best_score >= 90 {
        println!("VERDICT: ✅ working variant — {best_label}");
    } else if best_score > 0 {
        println!("VERDICT: ⚠️  best partial — {best_label} (no variant returned real bytes)");
    } else {
        println!("VERDICT: ❌ all variants failed");
    }
    println!("==================================================");
}
