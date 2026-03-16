use tree_sitter::Node;

use crate::index::{Language, node_text, truncate};

// Truncation constants (single source of truth)
pub(crate) const INSIGHT_CALL_TRUNCATE: usize = 40;
pub(crate) const INSIGHT_MATCH_TARGET_TRUNCATE: usize = 30;
pub(crate) const INSIGHT_ARM_TRUNCATE: usize = 30;
pub(crate) const INSIGHT_ERROR_TRUNCATE: usize = 40;
pub(crate) const MAX_CALLS: usize = 12;
pub(crate) const MAX_MATCH_ARMS: usize = 10;
pub(crate) const MAX_ERRORS: usize = 8;

#[derive(Debug, Clone, Default)]
pub(crate) struct MatchInsight {
    pub(crate) target: String,
    pub(crate) arms: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct BodyInsights {
    pub(crate) calls: Vec<String>,
    pub(crate) match_arms: Vec<MatchInsight>,
    pub(crate) error_returns: Vec<String>,
    pub(crate) try_count: usize,
}

impl BodyInsights {
    pub(crate) fn is_empty(&self) -> bool {
        self.calls.is_empty()
            && self.match_arms.is_empty()
            && self.error_returns.is_empty()
            && self.try_count == 0
    }

    pub(crate) fn format_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();

        // Calls (sorted lexicographically, truncated)
        if !self.calls.is_empty() {
            let display: Vec<&str> = self.calls.iter().take(MAX_CALLS).map(|s| s.as_str()).collect();
            let suffix = if self.calls.len() > MAX_CALLS { ", ..." } else { "" };
            lines.push(format!("→ calls: {}{suffix}", display.join(", ")));
        }

        // Match/switch arms (source order)
        for m in &self.match_arms {
            let arms_display: Vec<&str> = m.arms.iter().take(MAX_MATCH_ARMS).map(|s| s.as_str()).collect();
            let suffix = if m.arms.len() > MAX_MATCH_ARMS { ", ..." } else { "" };
            lines.push(format!("→ match: {} → {}{suffix}", m.target, arms_display.join(", ")));
        }

        // Errors (named first in source order, then ? count)
        if !self.error_returns.is_empty() || self.try_count > 0 {
            let mut parts: Vec<String> = self.error_returns
                .iter()
                .take(MAX_ERRORS)
                .cloned()
                .collect();
            if self.error_returns.len() > MAX_ERRORS {
                parts.push("...".to_string());
            }
            if self.try_count > 0 {
                parts.push(format!("{}× ?", self.try_count));
            }
            lines.push(format!("→ errors: {}", parts.join(", ")));
        }

        lines
    }
}

/// Analyze a function/method declaration node and extract body insights.
/// Pass the function declaration node itself (e.g., `function_item`), not the body.
/// Returns empty insights if the node has no body (abstract/interface methods).
pub(crate) fn analyze_body(node: Node, source: &[u8], lang: Language) -> BodyInsights {
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return BodyInsights::default(),
    };

    let calls = extract_calls(body, source, lang);
    let match_arms = extract_match_arms(body, source, lang);
    let (error_returns, try_count) = extract_error_returns(body, source, lang);

    BodyInsights {
        calls,
        match_arms,
        error_returns,
        try_count,
    }
}

// --- Placeholder implementations (filled in subsequent tasks) ---

fn extract_calls(_body: Node, _source: &[u8], _lang: Language) -> Vec<String> {
    Vec::new()
}

fn extract_match_arms(_body: Node, _source: &[u8], _lang: Language) -> Vec<MatchInsight> {
    Vec::new()
}

fn extract_error_returns(_body: Node, _source: &[u8], _lang: Language) -> (Vec<String>, usize) {
    (Vec::new(), 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_lines_empty() {
        let insights = BodyInsights::default();
        assert!(insights.is_empty());
        assert!(insights.format_lines().is_empty());
    }

    #[test]
    fn test_format_lines_calls_only() {
        let insights = BodyInsights {
            calls: vec!["bar".into(), "foo".into(), "qux".into()],
            ..Default::default()
        };
        assert!(!insights.is_empty());
        assert_eq!(insights.format_lines(), vec!["→ calls: bar, foo, qux"]);
    }

    #[test]
    fn test_format_lines_calls_truncated() {
        let calls: Vec<String> = (0..15).map(|i| format!("fn_{i}")).collect();
        let insights = BodyInsights {
            calls,
            ..Default::default()
        };
        let lines = insights.format_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with(", ..."));
        // Should contain exactly 12 function names
        assert_eq!(lines[0].matches(',').count(), 12); // 11 commas between 12 items + ", ..."
    }

    #[test]
    fn test_format_lines_match() {
        let insights = BodyInsights {
            match_arms: vec![MatchInsight {
                target: "cmd".into(),
                arms: vec!["\"start\"".into(), "\"stop\"".into(), "_".into()],
            }],
            ..Default::default()
        };
        let lines = insights.format_lines();
        assert_eq!(lines, vec!["→ match: cmd → \"start\", \"stop\", _"]);
    }

    #[test]
    fn test_format_lines_errors_with_try() {
        let insights = BodyInsights {
            error_returns: vec!["IoError".into(), "ParseError".into()],
            try_count: 3,
            ..Default::default()
        };
        let lines = insights.format_lines();
        assert_eq!(lines, vec!["→ errors: IoError, ParseError, 3× ?"]);
    }

    #[test]
    fn test_format_lines_try_only() {
        let insights = BodyInsights {
            try_count: 5,
            ..Default::default()
        };
        let lines = insights.format_lines();
        assert_eq!(lines, vec!["→ errors: 5× ?"]);
    }

    #[test]
    fn test_format_lines_all_sections() {
        let insights = BodyInsights {
            calls: vec!["alpha".into(), "beta".into()],
            match_arms: vec![MatchInsight {
                target: "x".into(),
                arms: vec!["1".into(), "2".into()],
            }],
            error_returns: vec!["Err(NotFound)".into()],
            try_count: 1,
        };
        let lines = insights.format_lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "→ calls: alpha, beta");
        assert_eq!(lines[1], "→ match: x → 1, 2");
        assert_eq!(lines[2], "→ errors: Err(NotFound), 1× ?");
    }
}
