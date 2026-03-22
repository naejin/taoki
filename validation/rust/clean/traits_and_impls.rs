// Expected: exit 0
// Expected: sections=imports,traits,impls,fns
// Expected: contains=Handler
// Expected: contains=Handler for ApiHandler

use std::io::Write;

pub trait Handler {
    fn handle(&self, input: &str) -> String;
}

pub struct ApiHandler {
    prefix: String,
}

impl Handler for ApiHandler {
    fn handle(&self, input: &str) -> String {
        format!("{}: {}", self.prefix, input)
    }
}

pub fn process(handler: &dyn Handler, data: &str) -> String {
    handler.handle(data)
}
