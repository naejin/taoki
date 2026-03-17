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

fn inject_content(content: &str, table: &str) -> Result<String, String> {
    let start = content
        .find(BENCH_START)
        .ok_or_else(|| format!("{} marker not found", BENCH_START))?;
    let end = content
        .find(BENCH_END)
        .ok_or_else(|| format!("{} marker not found", BENCH_END))?;

    let before = &content[..start + BENCH_START.len()];
    let after = &content[end..];
    Ok(format!("{}\n{}{}", before, table, after))
}

fn inject_readme(readme_path: &str, table: &str) -> Result<(), String> {
    let content = fs::read_to_string(readme_path)
        .map_err(|e| format!("failed to read {}: {}", readme_path, e))?;
    let new_content = inject_content(&content, table)?;
    fs::write(readme_path, new_content)
        .map_err(|e| format!("failed to write {}: {}", readme_path, e))?;
    Ok(())
}

fn format_table(results: &[ProjectResult]) -> String {
    let mut table = String::new();
    table.push_str(
        "| Project | Language | Files | Parsed | Parse % | Empty Skeletons | Reduction | Status |\n",
    );
    table.push_str(
        "|---------|----------|-------|--------|---------|-----------------|-----------|--------|\n",
    );
    for r in results {
        table.push_str(&format!(
            "| {} | {} | {} | {} | {:.0}% | {} | {:.0}% | {} |\n",
            r.name,
            r.lang,
            r.total_files,
            r.parsed_ok,
            r.parse_pct(),
            r.empty_skeletons,
            r.reduction_pct(),
            if r.passed() { "PASS" } else { "FAIL" },
        ));
    }
    table
}

fn format_error_summary(results: &[ProjectResult]) -> String {
    let mut all_errors: HashMap<String, usize> = HashMap::new();
    for r in results {
        for (msg, count) in &r.errors {
            *all_errors.entry(msg.clone()).or_insert(0) += count;
        }
    }
    if all_errors.is_empty() {
        return String::new();
    }
    let mut sorted: Vec<_> = all_errors.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted.truncate(10);

    let mut out = String::from("\nTop errors:\n");
    for (msg, count) in &sorted {
        out.push_str(&format!("  {}x {}\n", count, msg));
    }
    out
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

    #[test]
    fn error_summary_sorts_by_count() {
        let mut errors = HashMap::new();
        errors.insert("error A".into(), 3);
        errors.insert("error B".into(), 7);
        let results = vec![ProjectResult {
            errors,
            ..make_result(100, 10, 0, 50, 5000, 1000)
        }];
        let summary = format_error_summary(&results);
        let a_pos = summary.find("error A").unwrap();
        let b_pos = summary.find("error B").unwrap();
        assert!(b_pos < a_pos, "higher-count error should appear first");
    }

    #[test]
    fn error_summary_empty_when_no_errors() {
        let results = vec![make_result(100, 0, 0, 50, 5000, 1000)];
        assert!(format_error_summary(&results).is_empty());
    }

    #[test]
    fn format_table_produces_valid_markdown() {
        let results = vec![make_result(142, 0, 0, 80, 10000, 2200)];
        let table = format_table(&results);
        assert!(table.starts_with("| Project "));
        assert!(table.contains("| test |"));
        assert!(table.contains("| 142 |"));
        assert!(table.contains("| PASS |"));
    }

    #[test]
    fn inject_content_replaces_between_markers() {
        let content = "before\n<!-- BENCH:START -->\nold data\n<!-- BENCH:END -->\nafter";
        let result = inject_content(content, "| new | data |\n").unwrap();
        assert!(result.contains("| new | data |"));
        assert!(!result.contains("old data"));
        assert!(result.contains("before"));
        assert!(result.contains("after"));
    }

    #[test]
    fn inject_content_errors_on_missing_markers() {
        assert!(inject_content("no markers here", "table").is_err());
    }
}
