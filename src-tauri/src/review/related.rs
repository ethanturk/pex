use std::collections::{HashMap, HashSet};

pub fn related_file_groups(paths: &[String]) -> HashMap<String, Vec<String>> {
    let normalized: Vec<String> = paths.iter().map(|p| normalize_path(p)).collect();
    let mut by_path: HashMap<String, HashSet<String>> = normalized
        .iter()
        .map(|p| (p.clone(), HashSet::new()))
        .collect();

    for i in 0..normalized.len() {
        for j in (i + 1)..normalized.len() {
            let a = &normalized[i];
            let b = &normalized[j];
            if are_related(a, b) {
                by_path.entry(a.clone()).or_default().insert(b.clone());
                by_path.entry(b.clone()).or_default().insert(a.clone());
            }
        }
    }

    by_path
        .into_iter()
        .map(|(path, set)| {
            let mut related: Vec<String> = set.into_iter().collect();
            related.sort();
            (path, related)
        })
        .collect()
}

fn are_related(a: &str, b: &str) -> bool {
    same_basename(a, b)
        || source_test_pair(a, b)
        || interface_implementation_pair(a, b)
        || config_schema_pair(a, b)
}

fn same_basename(a: &str, b: &str) -> bool {
    stem(a) == stem(b) && stem(a).len() > 2
}

fn source_test_pair(a: &str, b: &str) -> bool {
    canonical_test_stem(a) == canonical_test_stem(b) && is_test_path(a) != is_test_path(b)
}

fn interface_implementation_pair(a: &str, b: &str) -> bool {
    let a_lower = a.to_ascii_lowercase();
    let b_lower = b.to_ascii_lowercase();
    let same = stem(a) == stem(b);
    same && ((a_lower.ends_with(".d.ts") && b_lower.ends_with(".ts"))
        || (b_lower.ends_with(".d.ts") && a_lower.ends_with(".ts"))
        || (a_lower.ends_with(".h") && (b_lower.ends_with(".c") || b_lower.ends_with(".cpp")))
        || (b_lower.ends_with(".h") && (a_lower.ends_with(".c") || a_lower.ends_with(".cpp"))))
}

fn config_schema_pair(a: &str, b: &str) -> bool {
    let a_base = basename(a).to_ascii_lowercase();
    let b_base = basename(b).to_ascii_lowercase();
    (a_base.contains("config") && b_base.contains("schema"))
        || (a_base.contains("schema") && b_base.contains("config"))
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("__tests__")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_test.rs")
}

fn canonical_test_stem(path: &str) -> String {
    stem(path)
        .trim_end_matches("_test")
        .trim_end_matches(".test")
        .trim_end_matches(".spec")
        .to_string()
}

fn stem(path: &str) -> String {
    let base = basename(path);
    if let Some(stripped) = base.strip_suffix(".d.ts") {
        return stripped.to_string();
    }
    base.rsplit_once('.')
        .map(|(name, _)| name.to_string())
        .unwrap_or_else(|| base.to_string())
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn normalize_path(path: &str) -> String {
    path.trim_start_matches('/').replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_source_and_test_pairs() {
        let paths = vec![
            "src/lib/api.ts".to_string(),
            "src/lib/api.test.ts".to_string(),
            "src/app.tsx".to_string(),
        ];
        let groups = related_file_groups(&paths);
        assert_eq!(groups["src/lib/api.ts"], vec!["src/lib/api.test.ts"]);
    }

    #[test]
    fn groups_interface_and_implementation_pairs() {
        let paths = vec!["src/foo.d.ts".to_string(), "src/foo.ts".to_string()];
        let groups = related_file_groups(&paths);
        assert_eq!(groups["src/foo.ts"], vec!["src/foo.d.ts"]);
    }

    #[test]
    fn groups_config_and_schema_pairs() {
        let paths = vec![
            "schema/app.schema.json".to_string(),
            "src/app.config.ts".to_string(),
        ];
        let groups = related_file_groups(&paths);
        assert_eq!(groups["schema/app.schema.json"], vec!["src/app.config.ts"]);
    }
}
