use std::collections::HashMap;
use std::path::PathBuf;

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
}
