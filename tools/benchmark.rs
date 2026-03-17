use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command},
};

use serde::{Deserialize, Serialize};
use taoki::{
    codemap,
    index::{self, Language},
    mcp,
};

const REPOS_JSON: &str = "tools/repos.json";
const CACHE_DIR: &str = "tools/.cache/repos";
const README_PATH: &str = "README.md";
const BENCH_START: &str = "<!-- BENCH:START -->";
const BENCH_END: &str = "<!-- BENCH:END -->";

const PARSE_THRESHOLD: f64 = 99.5;
const EMPTY_THRESHOLD: f64 = 1.0;
const REDUCTION_THRESHOLD: f64 = 50.0;
const MIN_LINES: usize = 50;
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
struct RepoEntry {
    name: String,
    url: String,
    sha: String,
    lang: String,
}

struct ProjectResult {
    name: String,
    lang: String,
    total_files: usize,
    parsed_ok: usize,
    parse_errors: usize,
    skipped: usize,
    empty_skeletons: usize,
    substantial_files: usize,
    total_source_bytes: usize,
    total_skeleton_bytes: usize,
    errors: HashMap<String, usize>,
}

impl ProjectResult {
    fn parse_pct(&self) -> f64 {
        let attempted = self.parsed_ok + self.parse_errors;
        if attempted == 0 {
            return 100.0;
        }
        self.parsed_ok as f64 / attempted as f64 * 100.0
    }

    fn empty_pct(&self) -> f64 {
        if self.substantial_files == 0 {
            return 0.0;
        }
        self.empty_skeletons as f64 / self.substantial_files as f64 * 100.0
    }

    fn reduction_pct(&self) -> f64 {
        if self.total_source_bytes == 0 {
            return 100.0;
        }
        (1.0 - self.total_skeleton_bytes as f64 / self.total_source_bytes as f64) * 100.0
    }

    fn passed(&self) -> bool {
        self.parse_pct() >= PARSE_THRESHOLD
            && self.empty_pct() <= EMPTY_THRESHOLD
            && self.reduction_pct() >= REDUCTION_THRESHOLD
    }
}

fn validate_repos(repos: &[RepoEntry]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen_names: HashMap<&str, usize> = HashMap::new();
    for (i, repo) in repos.iter().enumerate() {
        if repo.name.is_empty() {
            errors.push(format!("entry {}: missing name", i));
        }
        if repo.url.is_empty() {
            errors.push(format!("entry {} ({}): missing url", i, repo.name));
        }
        if let Some(prev) = seen_names.insert(&repo.name, i) {
            errors.push(format!(
                "entry {} ({}): duplicate name (first seen at entry {})",
                i, repo.name, prev
            ));
        }
    }
    errors
}

fn main() {
    println!("benchmark stub");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_catches_duplicate_names() {
        let repos = vec![
            RepoEntry { name: "foo".into(), url: "https://x.com/foo".into(), sha: "abc".into(), lang: "Rust".into() },
            RepoEntry { name: "foo".into(), url: "https://x.com/bar".into(), sha: "def".into(), lang: "Go".into() },
        ];
        let errors = validate_repos(&repos);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("duplicate name"));
    }

    #[test]
    fn validate_catches_missing_url() {
        let repos = vec![
            RepoEntry { name: "foo".into(), url: "".into(), sha: "abc".into(), lang: "Rust".into() },
        ];
        let errors = validate_repos(&repos);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing url"));
    }

    #[test]
    fn validate_accepts_valid_repos() {
        let repos = vec![
            RepoEntry { name: "a".into(), url: "https://a.com".into(), sha: "abc".into(), lang: "Rust".into() },
            RepoEntry { name: "b".into(), url: "https://b.com".into(), sha: "def".into(), lang: "Go".into() },
        ];
        assert!(validate_repos(&repos).is_empty());
    }

    fn make_result(
        parsed_ok: usize,
        parse_errors: usize,
        empty_skeletons: usize,
        substantial: usize,
        source_bytes: usize,
        skeleton_bytes: usize,
    ) -> ProjectResult {
        ProjectResult {
            name: "test".into(),
            lang: "Rust".into(),
            total_files: parsed_ok + parse_errors,
            parsed_ok,
            parse_errors,
            skipped: 0,
            empty_skeletons,
            substantial_files: substantial,
            total_source_bytes: source_bytes,
            total_skeleton_bytes: skeleton_bytes,
            errors: HashMap::new(),
        }
    }

    #[test]
    fn pass_all_thresholds() {
        let r = make_result(200, 0, 0, 100, 10000, 2000);
        assert!(r.passed());
        assert_eq!(r.parse_pct(), 100.0);
        assert_eq!(r.empty_pct(), 0.0);
        assert_eq!(r.reduction_pct(), 80.0);
    }

    #[test]
    fn pass_at_exact_parse_boundary() {
        // 199/200 = 99.5% — exactly at threshold
        let r = make_result(199, 1, 0, 100, 10000, 2000);
        assert!(r.passed());
    }

    #[test]
    fn fail_below_parse_threshold() {
        // 198/200 = 99.0%
        let r = make_result(198, 2, 0, 100, 10000, 2000);
        assert!(!r.passed());
    }

    #[test]
    fn pass_at_exact_empty_boundary() {
        // 1/100 = 1.0% — exactly at threshold
        let r = make_result(200, 0, 1, 100, 10000, 2000);
        assert!(r.passed());
    }

    #[test]
    fn fail_above_empty_threshold() {
        // 2/100 = 2.0%
        let r = make_result(200, 0, 2, 100, 10000, 2000);
        assert!(!r.passed());
    }

    #[test]
    fn pass_at_exact_reduction_boundary() {
        // 1 - 5000/10000 = 50%
        let r = make_result(200, 0, 0, 100, 10000, 5000);
        assert!(r.passed());
    }

    #[test]
    fn fail_below_reduction_threshold() {
        // 1 - 5100/10000 = 49%
        let r = make_result(200, 0, 0, 100, 10000, 5100);
        assert!(!r.passed());
    }

    #[test]
    fn pass_with_no_files() {
        let r = make_result(0, 0, 0, 0, 0, 0);
        assert!(r.passed());
    }
}
