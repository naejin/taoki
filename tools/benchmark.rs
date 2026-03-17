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

fn update_pins(repos_path: &str) {
    let data = fs::read_to_string(repos_path).expect("failed to read repos.json");
    let mut repos: Vec<RepoEntry> =
        serde_json::from_str(&data).expect("failed to parse repos.json");

    for repo in &mut repos {
        let output = Command::new("git")
            .args(["ls-remote", &repo.url, "HEAD"])
            .output()
            .expect("failed to run git ls-remote");
        if !output.status.success() {
            eprintln!("ERROR: git ls-remote failed for {}", repo.url);
            process::exit(1);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let sha = stdout
            .split_whitespace()
            .next()
            .expect("no SHA in ls-remote output");
        println!("{}: {} -> {}", repo.name, repo.sha, sha);
        repo.sha = sha.to_string();
    }

    let json = serde_json::to_string_pretty(&repos).expect("failed to serialize");
    fs::write(repos_path, json + "\n").expect("failed to write repos.json");
    println!("\nUpdated {} entries in {}", repos.len(), repos_path);
}

fn clone_or_fetch(url: &str, sha: &str, repo_path: &Path) {
    if !repo_path.exists() {
        let status = Command::new("git")
            .args(["clone", "--depth", "1", url])
            .arg(repo_path)
            .status()
            .expect("failed to run git clone");
        if !status.success() {
            eprintln!("ERROR: git clone failed for {}", url);
            process::exit(1);
        }
    }

    let fetch = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["fetch", "--depth", "1", "origin", sha])
        .status()
        .expect("failed to run git fetch");
    if !fetch.success() {
        eprintln!("ERROR: git fetch failed for {} (sha {})", url, sha);
        process::exit(1);
    }

    let checkout = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["checkout", sha])
        .status()
        .expect("failed to run git checkout");
    if !checkout.success() {
        eprintln!("ERROR: git checkout failed for sha {}", sha);
        process::exit(1);
    }
}

fn run_project(repo: &RepoEntry) -> ProjectResult {
    let repo_path = PathBuf::from(CACHE_DIR).join(&repo.name);
    clone_or_fetch(&repo.url, &repo.sha, &repo_path);

    let files = codemap::walk_files_public(&repo_path).unwrap_or_else(|e| {
        eprintln!("ERROR: failed to walk {}: {}", repo.name, e);
        process::exit(1);
    });

    let mut result = ProjectResult {
        name: repo.name.clone(),
        lang: repo.lang.clone(),
        total_files: 0,
        parsed_ok: 0,
        parse_errors: 0,
        skipped: 0,
        empty_skeletons: 0,
        substantial_files: 0,
        total_source_bytes: 0,
        total_skeleton_bytes: 0,
        errors: HashMap::new(),
    };

    for file in &files {
        let source = match fs::read(file) {
            Ok(s) => s,
            Err(_) => {
                result.skipped += 1;
                continue;
            }
        };

        if source.len() as u64 > MAX_FILE_SIZE {
            result.skipped += 1;
            continue;
        }

        if index::is_minified(&source) {
            result.skipped += 1;
            continue;
        }

        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = match Language::from_extension(ext) {
            Some(l) => l,
            None => {
                result.skipped += 1;
                continue;
            }
        };

        result.total_files += 1;

        match index::extract_all(&source, lang) {
            Ok((_api, skeleton)) => {
                result.parsed_ok += 1;
                result.total_source_bytes += source.len();
                result.total_skeleton_bytes += skeleton.len();

                let line_count = source.iter().filter(|&&b| b == b'\n').count();
                let is_test = mcp::is_test_filename(file);

                if line_count > MIN_LINES && !is_test {
                    result.substantial_files += 1;
                    if skeleton.trim().is_empty() {
                        result.empty_skeletons += 1;
                    }
                }
            }
            Err(e) => {
                result.parse_errors += 1;
                *result.errors.entry(e.to_string()).or_insert(0) += 1;
            }
        }
    }

    result
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

fn print_results(results: &[ProjectResult]) {
    println!("\n{:-<80}", "");
    println!("BENCHMARK RESULTS");
    println!("{:-<80}\n", "");

    for r in results {
        let status = if r.passed() { "PASS" } else { "FAIL" };
        println!(
            "{} [{}] - {} files, {:.1}% parsed, {} empty ({:.1}%), {:.1}% reduction - {}",
            r.name,
            r.lang,
            r.total_files,
            r.parse_pct(),
            r.empty_skeletons,
            r.empty_pct(),
            r.reduction_pct(),
            status,
        );
    }

    let error_summary = format_error_summary(results);
    if !error_summary.is_empty() {
        print!("{}", error_summary);
    }

    let all_pass = results.iter().all(|r| r.passed());
    println!("\n{:-<80}", "");
    println!("OVERALL: {}", if all_pass { "PASS" } else { "FAIL" });
    println!("{:-<80}", "");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "--update-pins") {
        update_pins(REPOS_JSON);
        return;
    }

    let data = fs::read_to_string(REPOS_JSON).expect("failed to read repos.json");
    let repos: Vec<RepoEntry> = serde_json::from_str(&data).expect("failed to parse repos.json");

    let validation_errors = validate_repos(&repos);
    if !validation_errors.is_empty() {
        eprintln!("repos.json validation errors:");
        for e in &validation_errors {
            eprintln!("  - {}", e);
        }
        process::exit(1);
    }

    let placeholder_repos: Vec<_> = repos
        .iter()
        .filter(|r| r.sha.starts_with('<') || r.sha.is_empty())
        .collect();
    if !placeholder_repos.is_empty() {
        eprintln!("repos.json has placeholder SHAs. Run with --update-pins first:");
        for r in &placeholder_repos {
            eprintln!("  - {} (sha: {})", r.name, r.sha);
        }
        process::exit(1);
    }

    fs::create_dir_all(CACHE_DIR).expect("failed to create cache dir");

    let mut results = Vec::new();
    for repo in &repos {
        println!("\n>>> Benchmarking: {} ({})", repo.name, repo.lang);
        results.push(run_project(repo));
    }

    print_results(&results);

    let table = format_table(&results);
    match inject_readme(README_PATH, &table) {
        Ok(()) => println!("\nREADME.md updated."),
        Err(e) => eprintln!("\nWARNING: {}", e),
    }

    let all_pass = results.iter().all(|r| r.passed());
    if !all_pass {
        process::exit(1);
    }
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
