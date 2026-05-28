use crate::ado::DiffResult;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct DiffCacheKey {
    pub org_url: String,
    pub project_id: String,
    pub repo_id: String,
    pub pr_id: i64,
    pub file_path: String,
    pub view: String,
    pub iteration: i32,
}

struct Entry {
    diff: DiffResult,
    inserted_at: Instant,
}

#[derive(Default)]
pub struct DiffCache {
    inner: Mutex<HashMap<DiffCacheKey, Entry>>,
}

impl DiffCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &DiffCacheKey) -> Option<DiffResult> {
        let mut map = self.inner.lock().unwrap();
        let entry = map.get(key)?;
        if entry.inserted_at.elapsed() >= TTL {
            map.remove(key);
            return None;
        }
        Some(entry.diff.clone())
    }

    pub fn put(&self, key: DiffCacheKey, diff: DiffResult) {
        let mut map = self.inner.lock().unwrap();
        // Lazy GC: drop any entries that have expired so the map can't grow
        // unboundedly during a long session.
        let now = Instant::now();
        map.retain(|_, e| now.duration_since(e.inserted_at) < TTL);
        map.insert(
            key,
            Entry {
                diff,
                inserted_at: now,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key() -> DiffCacheKey {
        DiffCacheKey {
            org_url: "o".into(),
            project_id: "p".into(),
            repo_id: "r".into(),
            pr_id: 1,
            file_path: "src/foo.rs".into(),
            view: "inline".into(),
            iteration: 1,
        }
    }

    fn sample_diff() -> DiffResult {
        DiffResult {
            html: "<div></div>".into(),
            path: "src/foo.rs".into(),
            status: "edit".into(),
            source_commit: "abc".into(),
            base_commit: Some("def".into()),
            old_content: "old".into(),
            new_content: "new".into(),
        }
    }

    #[test]
    fn miss_then_hit() {
        let c = DiffCache::new();
        let k = sample_key();
        assert!(c.get(&k).is_none());
        c.put(k.clone(), sample_diff());
        let got = c.get(&k).expect("cached value");
        assert_eq!(got.source_commit, "abc");
    }

    #[test]
    fn different_iteration_is_different_key() {
        let c = DiffCache::new();
        let mut k1 = sample_key();
        k1.iteration = 1;
        let mut k2 = sample_key();
        k2.iteration = 2;
        c.put(k1.clone(), sample_diff());
        assert!(c.get(&k2).is_none(), "new iteration must miss");
        assert!(c.get(&k1).is_some());
    }

    #[test]
    fn different_view_is_different_key() {
        let c = DiffCache::new();
        let mut k1 = sample_key();
        k1.view = "inline".into();
        let mut k2 = sample_key();
        k2.view = "split".into();
        c.put(k1, sample_diff());
        assert!(
            c.get(&k2).is_none(),
            "split view must not reuse inline cache"
        );
    }
}
