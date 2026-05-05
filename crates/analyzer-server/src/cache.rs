//! Two-layer cache for AI summaries: in-memory HashMap + on-disk JSON files.
//!
//! Why: every cache hit avoids a Groq API call, which costs latency and quota.
//! Identical inputs hash to the same key, so re-uploading the same log is free.
//!
//! Layout:
//!   cache/<sha256-prefix>.json   — one entry per file, atomic write via rename.

use crate::ai::AiSummary;
use analyzer_core::Stats;
use lru::LruCache;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Write,
    num::NonZeroUsize,
    path::PathBuf,
    sync::{Arc, Mutex},
};

const DEFAULT_CACHE_DIR: &str = "cache";
const KEY_PREFIX_LEN: usize = 16;
const MEM_CACHE_CAP: usize = 256;

#[derive(Clone)]
pub struct SummaryCache {
    mem: Arc<Mutex<LruCache<String, AiSummary>>>,
    dir: PathBuf,
}

impl SummaryCache {
    pub fn open() -> Self {
        let dir = std::env::var_os("CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_CACHE_DIR));
        let _ = fs::create_dir_all(&dir);
        Self {
            mem: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(MEM_CACHE_CAP).unwrap(),
            ))),
            dir,
        }
    }

    /// Deterministic key from the stats + model. Same input -> same key
    /// across processes, so on-disk hits survive restarts.
    pub fn key_for(stats: &Stats, model: &str) -> String {
        let normalized = serde_json::json!({
            "model": model,
            "total_lines": stats.total_lines,
            "parsed_lines": stats.parsed_lines,
            "requests": stats.requests,
            "bytes": stats.bytes,
            "by_status": sorted(&stats.by_status),
            "by_ip": sorted(&stats.by_ip),
            "by_path": sorted(&stats.by_path),
            "by_hour": sorted(&stats.by_hour),
        });
        let bytes = serde_json::to_vec(&normalized).unwrap_or_default();
        let digest = Sha256::digest(&bytes);
        hex_lower(&digest[..KEY_PREFIX_LEN])
    }

    pub fn get(&self, key: &str) -> Option<AiSummary> {
        if let Some(hit) = self.mem.lock().unwrap().get(key).cloned() {
            return Some(hit);
        }
        let path = self.path_for(key);
        let bytes = fs::read(&path).ok()?;
        let summary: AiSummary = serde_json::from_slice(&bytes).ok()?;
        self.mem.lock().unwrap().put(key.to_string(), summary.clone());
        Some(summary)
    }

    pub fn put(&self, key: &str, summary: &AiSummary) {
        self.mem.lock().unwrap().put(key.to_string(), summary.clone());
        let _ = self.write_atomic(key, summary);
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    fn write_atomic(&self, key: &str, summary: &AiSummary) -> std::io::Result<()> {
        let final_path = self.path_for(key);
        let tmp_path = self.dir.join(format!(".{key}.tmp"));
        let mut f = fs::File::create(&tmp_path)?;
        let bytes = serde_json::to_vec_pretty(summary)
            .map_err(std::io::Error::other)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        fs::rename(&tmp_path, &final_path)
    }
}

fn sorted<K: Ord + Clone, V: Clone>(m: &HashMap<K, V>) -> BTreeMap<K, V> {
    m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_stats_produces_same_key() {
        let mut s1 = Stats::default();
        s1.record("1.1.1.1".into(), 12, "/a".into(), 200, 100);
        s1.record("2.2.2.2".into(), 13, "/b".into(), 404, 50);

        let mut s2 = Stats::default();
        s2.record("2.2.2.2".into(), 13, "/b".into(), 404, 50);
        s2.record("1.1.1.1".into(), 12, "/a".into(), 200, 100);

        assert_eq!(
            SummaryCache::key_for(&s1, "test-model"),
            SummaryCache::key_for(&s2, "test-model"),
        );
    }

    #[test]
    fn different_model_changes_key() {
        let s = Stats::default();
        assert_ne!(
            SummaryCache::key_for(&s, "model-a"),
            SummaryCache::key_for(&s, "model-b"),
        );
    }
}
