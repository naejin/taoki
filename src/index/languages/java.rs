use tree_sitter::Node;
use crate::index::{LanguageExtractor, PublicApi, SkeletonEntry};

pub(crate) struct JavaExtractor;

impl LanguageExtractor for JavaExtractor {
    fn extract_nodes(&self, _node: Node, _source: &[u8], _attrs: &[Node]) -> Vec<SkeletonEntry> {
        Vec::new()
    }
    fn is_test_node(&self, _node: Node, _source: &[u8], _attrs: &[Node]) -> bool { false }
    fn is_doc_comment(&self, _node: Node, _source: &[u8]) -> bool { false }
    fn is_module_doc(&self, _node: Node, _source: &[u8]) -> bool { false }
    fn extract_public_api(&self, _root: Node, _source: &[u8]) -> PublicApi {
        PublicApi { types: Vec::new(), functions: Vec::new() }
    }
}
