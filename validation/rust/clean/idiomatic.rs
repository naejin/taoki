// Expected: exit 0
// Expected: sections=imports,types,fns
// Expected: contains=UserService
// Expected: contains=create_user
// Expected: contains=Config

use std::collections::HashMap;

/// Application configuration.
pub struct Config {
    pub host: String,
    pub port: u16,
}

/// Manages user operations.
pub struct UserService {
    db: HashMap<u32, String>,
}

impl UserService {
    pub fn new() -> Self {
        Self { db: HashMap::new() }
    }

    /// Create a new user.
    pub fn create_user(&mut self, id: u32, name: String) {
        self.db.insert(id, name);
    }

    pub fn get_user(&self, id: u32) -> Option<&String> {
        self.db.get(&id)
    }
}

fn internal_helper() -> bool {
    true
}
