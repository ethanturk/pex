use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct StandardsCacheKey {
    pub org_url: String,
    pub project_id: String,
    pub repo_id: String,
    pub commit: String,
    pub path: String,
}

struct Entry {
    /// `None` deliberately caches "file not found" so subsequent walks don't
    /// re-probe the same directory.
    content: Option<String>,
    inserted_at: Instant,
}

#[derive(Default)]
pub struct StandardsCache {
    inner: Mutex<HashMap<StandardsCacheKey, Entry>>,
}

impl StandardsCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &StandardsCacheKey) -> Option<Option<String>> {
        let mut map = self.inner.lock().unwrap();
        let entry = map.get(key)?;
        if entry.inserted_at.elapsed() >= TTL {
            map.remove(key);
            return None;
        }
        Some(entry.content.clone())
    }

    pub fn put(&self, key: StandardsCacheKey, content: Option<String>) {
        let mut map = self.inner.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, e| now.duration_since(e.inserted_at) < TTL);
        map.insert(
            key,
            Entry {
                content,
                inserted_at: now,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(path: &str) -> StandardsCacheKey {
        StandardsCacheKey {
            org_url: "o".into(),
            project_id: "p".into(),
            repo_id: "r".into(),
            commit: "c".into(),
            path: path.into(),
        }
    }

    #[test]
    fn miss_then_hit_for_some() {
        let c = StandardsCache::new();
        assert!(c.get(&key("AGENTS.md")).is_none());
        c.put(key("AGENTS.md"), Some("be careful".into()));
        let got = c.get(&key("AGENTS.md")).expect("entry");
        assert_eq!(got.as_deref(), Some("be careful"));
    }

    #[test]
    fn miss_is_cached_as_none() {
        let c = StandardsCache::new();
        c.put(key("AGENTS.md"), None);
        // Outer Some = cache hit; inner None = remembered "not found".
        let got = c.get(&key("AGENTS.md")).expect("cache hit");
        assert!(got.is_none());
    }
}
