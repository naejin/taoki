use std::collections::HashMap;
use std::fmt;
use std::io::{self, Read, Write};

pub const MAX_RETRIES: usize = 3;
pub const DEFAULT_TIMEOUT_MS: u64 = 5000;

/// Error types for the service layer.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

/// Configuration for the HTTP client.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub base_url: String,
    pub timeout_ms: u64,
    pub max_retries: usize,
    pub headers: HashMap<String, String>,
    pub user_agent: String,
    pub follow_redirects: bool,
    pub verify_ssl: bool,
    pub proxy: Option<String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            max_retries: MAX_RETRIES,
            headers: HashMap::new(),
            user_agent: "taoki/0.1".to_string(),
            follow_redirects: true,
            verify_ssl: true,
            proxy: None,
        }
    }
}

/// A paginated response wrapper.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub page: usize,
    pub per_page: usize,
}

impl<T: fmt::Display> fmt::Display for Page<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Page {}/{}", self.page, self.total.div_ceil(self.per_page))
    }
}

/// Trait for items that can be indexed.
pub trait Indexable {
    fn id(&self) -> &str;
    fn kind(&self) -> &str;
    fn score(&self) -> f64;
}

/// Trait for serializable data.
pub trait Serialize {
    fn to_json(&self) -> String;
    fn to_bytes(&self) -> Vec<u8>;
}

/// A user record.
#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    Admin,
    Editor,
    Viewer,
}

impl Indexable for User {
    fn id(&self) -> &str { &self.id }
    fn kind(&self) -> &str { "user" }
    fn score(&self) -> f64 { 1.0 }
}

pub fn fetch_user(id: &str) -> Result<User, ServiceError> {
    if id.is_empty() {
        return Err(ServiceError::InvalidInput("id cannot be empty".to_string()));
    }
    Ok(User {
        id: id.to_string(),
        name: "Alice".to_string(),
        email: "alice@example.com".to_string(),
        role: Role::Viewer,
    })
}

pub fn paginate<T>(items: Vec<T>, page: usize, per_page: usize) -> Page<T> {
    let total = items.len();
    let start = page.saturating_sub(1) * per_page;
    let end = (start + per_page).min(total);
    Page { items: items[start..end].to_vec(), total, page, per_page }
}

pub(crate) fn parse_header(raw: &str) -> Option<(String, String)> {
    raw.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
}

fn internal_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

macro_rules! impl_display {
    ($t:ty, $fmt:expr) => {
        impl fmt::Display for $t {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, $fmt, self)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_user_ok() {
        let u = fetch_user("u1").unwrap();
        assert_eq!(u.id, "u1");
    }

    #[test]
    fn fetch_user_empty_id() {
        assert!(fetch_user("").is_err());
    }

    #[test]
    fn paginate_basic() {
        let items: Vec<i32> = (0..10).collect();
        let page = paginate(items, 2, 3);
        assert_eq!(page.items, vec![3, 4, 5]);
    }
}
