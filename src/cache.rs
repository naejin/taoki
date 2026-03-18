use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Single source of truth for all cache format versions.
/// Bump this when ANY cache format changes — all caches invalidate together,
/// which is safe because rebuilds are fast and correct.
pub const CACHE_VERSION: u32 = 3;

/// Compute a blake3 fingerprint over the file list and workspace configuration.
/// This fingerprint changes when files are added/removed/renamed, or when
/// Cargo.toml/go.mod workspace configuration changes.
pub fn compute_fingerprint(
    all_files: &[String],
    crate_map: &HashMap<String, PathBuf>,
    go_module_map: &HashMap<String, PathBuf>,
) -> String {
    let mut hasher = blake3::Hasher::new();

    // Hash sorted file list
    let mut sorted_files: Vec<&str> = all_files.iter().map(|s| s.as_str()).collect();
    sorted_files.sort();
    for f in &sorted_files {
        hasher.update(f.as_bytes());
        hasher.update(b"\0");
    }

    // Hash crate map (sorted for determinism)
    hasher.update(b"\nCRATES\n");
    let mut crates: Vec<(&String, &PathBuf)> = crate_map.iter().collect();
    crates.sort_by_key(|(k, _)| k.as_str());
    for (name, dir) in &crates {
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(dir.to_string_lossy().as_bytes());
        hasher.update(b"\0");
    }

    // Hash go module map (sorted for determinism)
    hasher.update(b"\nGOMODS\n");
    let mut mods: Vec<(&String, &PathBuf)> = go_module_map.iter().collect();
    mods.sort_by_key(|(k, _)| k.as_str());
    for (path, dir) in &mods {
        hasher.update(path.as_bytes());
        hasher.update(b"=");
        hasher.update(dir.to_string_lossy().as_bytes());
        hasher.update(b"\0");
    }

    hasher.finalize().to_hex().to_string()
}

/// Remove xray cache entries for files that no longer exist in the repo.
/// Called during radar (which already walks the full file tree) to prevent
/// unbounded cache growth from deleted/renamed files.
pub fn prune_xray_cache(root: &Path, current_files: &[String]) {
    let mut cache = crate::mcp::load_xray_cache(root);
    let before = cache.files.len();

    let live: HashSet<&str> = current_files.iter().map(|s| s.as_str()).collect();
    cache.files.retain(|key, _| live.contains(key.as_str()));

    // Only write if something was actually pruned
    if cache.files.len() < before {
        crate::mcp::save_xray_cache(root, &cache);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_changes_on_file_list_change() {
        let crate_map = HashMap::new();
        let go_map = HashMap::new();
        let files1 = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let files2 = vec!["src/a.rs".to_string(), "src/b.rs".to_string(), "src/c.rs".to_string()];
        let fp1 = compute_fingerprint(&files1, &crate_map, &go_map);
        let fp2 = compute_fingerprint(&files2, &crate_map, &go_map);
        assert_ne!(fp1, fp2, "adding a file should change fingerprint");
    }

    #[test]
    fn fingerprint_stable_when_nothing_changes() {
        let crate_map = HashMap::new();
        let go_map = HashMap::new();
        let files = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        let fp1 = compute_fingerprint(&files, &crate_map, &go_map);
        let fp2 = compute_fingerprint(&files, &crate_map, &go_map);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_changes_on_crate_map_change() {
        let go_map = HashMap::new();
        let files = vec!["src/a.rs".to_string()];
        let mut crate_map1 = HashMap::new();
        crate_map1.insert("my_crate".to_string(), PathBuf::from("crates/my-crate"));
        let crate_map2 = HashMap::new();
        let fp1 = compute_fingerprint(&files, &crate_map1, &go_map);
        let fp2 = compute_fingerprint(&files, &crate_map2, &go_map);
        assert_ne!(fp1, fp2, "crate map change should change fingerprint");
    }

    #[test]
    fn fingerprint_order_independent() {
        let crate_map = HashMap::new();
        let go_map = HashMap::new();
        let files1 = vec!["src/b.rs".to_string(), "src/a.rs".to_string()];
        let files2 = vec!["src/a.rs".to_string(), "src/b.rs".to_string()];
        assert_eq!(
            compute_fingerprint(&files1, &crate_map, &go_map),
            compute_fingerprint(&files2, &crate_map, &go_map),
        );
    }

    #[test]
    fn fingerprint_changes_on_go_module_map_change() {
        let crate_map = HashMap::new();
        let files = vec!["cmd/main.go".to_string()];
        let mut go_map1 = HashMap::new();
        go_map1.insert("github.com/owner/repo".to_string(), PathBuf::from(""));
        let go_map2 = HashMap::new();
        let fp1 = compute_fingerprint(&files, &crate_map, &go_map1);
        let fp2 = compute_fingerprint(&files, &crate_map, &go_map2);
        assert_ne!(fp1, fp2, "go module map change should change fingerprint");
    }

    #[test]
    fn prune_xray_cache_removes_dead_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let cache_dir = dir.path().join(".cache/taoki");
        std::fs::create_dir_all(&cache_dir).unwrap();

        // Write a cache with 3 entries: 2 real files, 1 dead
        let cache_data = serde_json::json!({
            "version": CACHE_VERSION,
            "files": {
                "src/alive.rs": { "hash": "abc", "skeleton": "fn alive()" },
                "src/also_alive.rs": { "hash": "def", "skeleton": "fn also()" },
                "src/dead.rs": { "hash": "ghi", "skeleton": "fn dead()" }
            }
        });
        std::fs::write(cache_dir.join("xray.json"), serde_json::to_string(&cache_data).unwrap()).unwrap();

        let current_files = vec!["src/alive.rs".to_string(), "src/also_alive.rs".to_string()];
        prune_xray_cache(dir.path(), &current_files);

        // Reload and verify
        let data = std::fs::read_to_string(cache_dir.join("xray.json")).unwrap();
        let cache: serde_json::Value = serde_json::from_str(&data).unwrap();
        let files = cache["files"].as_object().unwrap();
        assert_eq!(files.len(), 2, "dead entry should be removed: {files:?}");
        assert!(files.contains_key("src/alive.rs"));
        assert!(files.contains_key("src/also_alive.rs"));
        assert!(!files.contains_key("src/dead.rs"));
    }
}
