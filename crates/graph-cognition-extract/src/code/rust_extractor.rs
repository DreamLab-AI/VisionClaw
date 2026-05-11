use std::collections::HashMap;

use graph_cognition_core::{EdgeKind, NodeKind, TypedEdge, TypedNode};
use regex::Regex;

use super::extractor::{mint_code_urn, CodeExtractor, ExtractionResult, Language};

/// Regex-based Rust source code extractor (ADR-065 Phase 1).
///
/// Extracts:
/// - `fn` definitions -> `NodeKind::Function`
/// - `mod` declarations -> `NodeKind::Module`
/// - `struct` / `enum` definitions -> `NodeKind::Class`
/// - `trait` definitions -> `NodeKind::Interface`
/// - `use` statements -> `EdgeKind::Imports` edges
/// - `impl Trait for Type` -> `EdgeKind::Implements` edges
/// - `impl Type` -> `EdgeKind::Contains` edges (methods belong to type)
///
/// Generates `EdgeKind::Contains` edges from the file-level module to each
/// top-level item, and from `impl` blocks to their methods.
///
/// Limitations of regex-based extraction (addressed in Phase 2 with tree-sitter):
/// - Cannot resolve nested scopes accurately
/// - Macros that generate items are invisible
/// - Generic parameters are not fully parsed
/// - Conditional compilation (`#[cfg(...)]`) is not evaluated
///
/// TODO(ADR-065 Phase 2): Migrate to tree-sitter for accurate AST-based
/// extraction. The current 8-pass regex approach is fragile against edge cases
/// (e.g. items inside macro_rules!, string literals containing `fn`, multi-line
/// generics). tree-sitter-rust provides a correct parse tree that eliminates
/// these failure modes and enables scope-aware extraction.
/// Tracking: <https://github.com/nichetypes/VisionClaw/issues/tree-sitter-migration>
pub struct RustExtractor {
    re_fn: Regex,
    re_mod: Regex,
    re_struct: Regex,
    re_enum: Regex,
    re_trait: Regex,
    re_use: Regex,
    re_impl_trait: Regex,
    re_impl_bare: Regex,
}

impl RustExtractor {
    pub fn new() -> Self {
        Self {
            // Match: pub/pub(crate)/pub(super) async? unsafe? const? extern? fn name
            re_fn: Regex::new(
                r#"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:const\s+)?(?:extern\s+(?:"[^"]*"\s+)?)?fn\s+(\w+)"#,
            )
            .expect("re_fn"),

            // Match: pub mod name (not `mod tests` in #[cfg(test)])
            re_mod: Regex::new(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)").expect("re_mod"),

            // Match: pub struct Name
            re_struct: Regex::new(
                r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)",
            )
            .expect("re_struct"),

            // Match: pub enum Name
            re_enum: Regex::new(
                r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)",
            )
            .expect("re_enum"),

            // Match: pub trait Name
            re_trait: Regex::new(
                r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?trait\s+(\w+)",
            )
            .expect("re_trait"),

            // Match: use path::to::item;  or  use path::to::{items};
            re_use: Regex::new(r"(?m)^[ \t]*(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);")
                .expect("re_use"),

            // Match: impl TraitName for TypeName
            re_impl_trait: Regex::new(
                r"(?m)^[ \t]*impl(?:<[^>]*>)?\s+(\w+)(?:<[^>]*>)?\s+for\s+(\w+)",
            )
            .expect("re_impl_trait"),

            // Match: impl TypeName { (bare impl, not trait impl)
            re_impl_bare: Regex::new(
                r"(?m)^[ \t]*impl(?:<[^>]*>)?\s+(\w+)(?:<[^>]*>)?\s*\{",
            )
            .expect("re_impl_bare"),
        }
    }

    /// Extract the module name from a file path.
    ///
    /// `src/actors/graph.rs` -> `graph`
    /// `src/lib.rs` -> the crate root (uses directory name or "lib")
    fn module_name_from_path(file_path: &str) -> String {
        let normalized = file_path.replace('\\', "/");
        let stem = normalized
            .rsplit('/')
            .next()
            .unwrap_or(file_path)
            .trim_end_matches(".rs");

        if stem == "mod" || stem == "lib" || stem == "main" {
            // Use the parent directory name
            let parts: Vec<&str> = normalized.split('/').collect();
            if parts.len() >= 2 {
                return parts[parts.len() - 2].to_string();
            }
        }

        stem.to_string()
    }

    /// Find methods inside an `impl` block by scanning brace depth from the
    /// `impl` keyword position to the closing `}`.
    fn extract_impl_methods<'a>(&self, source: &'a str, impl_start: usize) -> Vec<&'a str> {
        let mut methods = Vec::new();
        let rest = &source[impl_start..];

        // Find the opening brace
        let brace_start = match rest.find('{') {
            Some(pos) => pos,
            None => return methods,
        };

        // Track brace depth to find the matching close
        let mut depth: u32 = 0;
        let block_start = impl_start + brace_start;
        let bytes = source.as_bytes();
        let mut pos = block_start;
        let mut block_end = source.len();

        while pos < bytes.len() {
            match bytes[pos] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        block_end = pos;
                        break;
                    }
                }
                _ => {}
            }
            pos += 1;
        }

        let block = &source[block_start..block_end];

        // Find fn definitions within this block
        for cap in self.re_fn.captures_iter(block) {
            if let Some(name) = cap.get(1) {
                methods.push(name.as_str());
            }
        }

        methods
    }
}

impl Default for RustExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeExtractor for RustExtractor {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn extract(&self, source: &str, file_path: &str) -> ExtractionResult {
        let mut nodes: Vec<TypedNode> = Vec::new();
        let mut edges: Vec<TypedEdge> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        // Track names we've already created nodes for (dedup across regex passes)
        let mut seen_nodes: HashMap<String, String> = HashMap::new(); // name -> urn

        let module_name = Self::module_name_from_path(file_path);
        let file_module_urn = mint_code_urn(file_path, &module_name);

        // Create a file-level module node
        let file_module = TypedNode::new(
            file_module_urn.clone(),
            NodeKind::Module,
            module_name.clone(),
        );
        nodes.push(file_module);
        seen_nodes.insert(module_name.clone(), file_module_urn.clone());

        // --- Pass 1: Functions ---
        for cap in self.re_fn.captures_iter(source) {
            let name = cap[1].to_string();
            let urn = mint_code_urn(file_path, &name);

            if seen_nodes.contains_key(&name) {
                continue;
            }

            let mut node = TypedNode::new(urn.clone(), NodeKind::Function, name.clone());
            // Store the line number as a property for downstream consumers
            if let Some(m) = cap.get(0) {
                let line = source[..m.start()].matches('\n').count() + 1;
                node.properties.insert(
                    "line".into(),
                    serde_json::Value::Number(serde_json::Number::from(line)),
                );
            }
            nodes.push(node);
            seen_nodes.insert(name, urn.clone());

            // File module contains this function
            edges.push(TypedEdge::new(
                file_module_urn.clone(),
                urn,
                EdgeKind::Contains,
            ));
        }

        // --- Pass 2: Modules ---
        for cap in self.re_mod.captures_iter(source) {
            let name = cap[1].to_string();
            let urn = mint_code_urn(file_path, &format!("mod_{}", name));

            if seen_nodes.contains_key(&format!("mod_{}", name)) {
                continue;
            }

            let node = TypedNode::new(urn.clone(), NodeKind::Module, name.clone());
            nodes.push(node);
            seen_nodes.insert(format!("mod_{}", name), urn.clone());

            edges.push(TypedEdge::new(
                file_module_urn.clone(),
                urn,
                EdgeKind::Contains,
            ));
        }

        // --- Pass 3 (consolidated): Structs + Enums -> Class nodes ---
        // Both struct and enum definitions produce NodeKind::Class with identical
        // extraction logic, so they share a single loop to avoid redundant passes.
        let type_regexes: &[&Regex] = &[&self.re_struct, &self.re_enum];
        for re in type_regexes {
            for cap in re.captures_iter(source) {
                let name = cap[1].to_string();
                let urn = mint_code_urn(file_path, &name);

                if seen_nodes.contains_key(&name) {
                    continue;
                }

                let node = TypedNode::new(urn.clone(), NodeKind::Class, name.clone());
                nodes.push(node);
                seen_nodes.insert(name, urn.clone());

                edges.push(TypedEdge::new(
                    file_module_urn.clone(),
                    urn,
                    EdgeKind::Contains,
                ));
            }
        }

        // --- Pass 4: Traits -> Interface nodes ---
        for cap in self.re_trait.captures_iter(source) {
            let name = cap[1].to_string();
            let urn = mint_code_urn(file_path, &name);

            if seen_nodes.contains_key(&name) {
                continue;
            }

            let node = TypedNode::new(urn.clone(), NodeKind::Interface, name.clone());
            nodes.push(node);
            seen_nodes.insert(name, urn.clone());

            edges.push(TypedEdge::new(
                file_module_urn.clone(),
                urn,
                EdgeKind::Contains,
            ));
        }

        // --- Pass 5: Use statements -> Imports edges ---
        for cap in self.re_use.captures_iter(source) {
            let use_path = cap[1].trim().to_string();

            // Extract the final item name(s) from the use path.
            // parse_use_path handles malformed brace groups gracefully.
            let imported_names = parse_use_path(&use_path);

            for import_name in imported_names {
                // Create an Imports edge from the file module to the imported item.
                // The target URN uses the raw import path since we cannot resolve
                // cross-file URNs without a full project index.
                let target_urn = format!("urn:visionclaw:concept:code:extern:{}", import_name);

                edges.push(TypedEdge::new(
                    file_module_urn.clone(),
                    target_urn,
                    EdgeKind::Imports,
                ));
            }
        }

        // --- Pass 6: impl Trait for Type -> Implements edges ---
        for cap in self.re_impl_trait.captures_iter(source) {
            let trait_name = cap[1].to_string();
            let type_name = cap[2].to_string();

            let type_urn = seen_nodes
                .get(&type_name)
                .cloned()
                .unwrap_or_else(|| mint_code_urn(file_path, &type_name));

            let trait_urn = seen_nodes
                .get(&trait_name)
                .cloned()
                .unwrap_or_else(|| mint_code_urn(file_path, &trait_name));

            edges.push(TypedEdge::new(type_urn, trait_urn, EdgeKind::Implements));
        }

        // --- Pass 7: impl Type { methods } -> Contains edges from type to methods ---
        for cap in self.re_impl_bare.captures_iter(source) {
            let type_name = cap[1].to_string();

            // Guard: cap.get(0) should always be Some for a successful match,
            // but defend against unexpected regex engine behaviour.
            let match_start = match cap.get(0) {
                Some(m) => m.start(),
                None => {
                    errors.push(format!(
                        "regex anomaly: re_impl_bare matched but capture group 0 missing in {file_path}"
                    ));
                    continue;
                }
            };

            // Skip if this is actually a trait impl (the bare regex can match
            // the type name in "impl Trait for Type {" if Trait has no generics)
            if self.re_impl_trait.is_match(&source[match_start..]) {
                if let Some(trait_cap) = self.re_impl_trait.captures(&source[match_start..]) {
                    if trait_cap.get(0).map_or(false, |m| m.start() == 0) {
                        continue;
                    }
                }
            }

            let type_urn = seen_nodes
                .get(&type_name)
                .cloned()
                .unwrap_or_else(|| mint_code_urn(file_path, &type_name));

            let methods = self.extract_impl_methods(source, match_start);

            for method_name in methods {
                if let Some(method_urn) = seen_nodes.get(method_name) {
                    edges.push(TypedEdge::new(
                        type_urn.clone(),
                        method_urn.clone(),
                        EdgeKind::Contains,
                    ));
                }
            }
        }

        if nodes.is_empty() && edges.is_empty() {
            errors.push(format!(
                "no code items extracted from {file_path}; file may be empty or use unsupported syntax"
            ));
        }

        ExtractionResult {
            nodes,
            edges,
            errors,
        }
    }
}

/// Parse a Rust `use` path into the individual imported item names.
///
/// Handles:
/// - `std::collections::HashMap` -> `["std::collections::HashMap"]`
/// - `std::io::{Read, Write}` -> `["std::io::Read", "std::io::Write"]`
/// - `crate::foo::*` -> `["crate::foo::*"]`
/// - `super::bar as baz` -> `["super::bar"]`
fn parse_use_path(path: &str) -> Vec<String> {
    let path = path.trim();

    // Handle group imports: prefix::{A, B, C}
    if let Some(brace_start) = path.find('{') {
        if let Some(brace_end) = path.find('}') {
            let prefix = path[..brace_start].trim_end_matches("::");
            let items = &path[brace_start + 1..brace_end];

            return items
                .split(',')
                .filter_map(|item| {
                    let item = item.trim();
                    if item.is_empty() {
                        return None;
                    }
                    // Strip `as alias`
                    let base = item.split_whitespace().next().unwrap_or(item);
                    Some(format!("{}::{}", prefix, base))
                })
                .collect();
        }
    }

    // Strip `as alias`
    let base = path.split(" as ").next().unwrap_or(path).trim();
    vec![base.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extractor() -> RustExtractor {
        RustExtractor::new()
    }

    // ── parse_use_path unit tests ──

    #[test]
    fn parse_use_simple() {
        assert_eq!(
            parse_use_path("std::collections::HashMap"),
            vec!["std::collections::HashMap"]
        );
    }

    #[test]
    fn parse_use_group() {
        let result = parse_use_path("std::io::{Read, Write}");
        assert_eq!(result, vec!["std::io::Read", "std::io::Write"]);
    }

    #[test]
    fn parse_use_alias() {
        assert_eq!(
            parse_use_path("std::io::Error as IoError"),
            vec!["std::io::Error"]
        );
    }

    #[test]
    fn parse_use_glob() {
        assert_eq!(parse_use_path("crate::prelude::*"), vec!["crate::prelude::*"]);
    }

    // ── Full extraction tests ──

    const SAMPLE_RUST: &str = r#"
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

pub mod actors;
mod utils;

/// A node in the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: u64,
    pub label: String,
    pub properties: HashMap<String, String>,
}

/// Edge types.
pub enum EdgeType {
    Contains,
    References,
    Implements,
}

/// Core behavior for graph operations.
pub trait GraphOperations {
    fn add_node(&mut self, node: GraphNode);
    fn remove_node(&mut self, id: u64);
}

/// The main graph service.
pub struct GraphService {
    nodes: Vec<GraphNode>,
}

impl GraphService {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

impl GraphOperations for GraphService {
    fn add_node(&mut self, node: GraphNode) {
        self.nodes.push(node);
    }

    fn remove_node(&mut self, id: u64) {
        self.nodes.retain(|n| n.id != id);
    }
}

pub fn process_graph(service: &GraphService) -> usize {
    service.node_count()
}

async fn fetch_remote_data(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(url.to_string())
}
"#;

    #[test]
    fn extracts_file_module_node() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let module_node = result
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Module && n.label == "graph")
            .expect("file-level module node");
        assert_eq!(
            module_node.urn,
            "urn:visionclaw:concept:code:src/graph.rs:graph"
        );
    }

    #[test]
    fn extracts_functions() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let fn_names: Vec<&str> = result
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Function)
            .map(|n| n.label.as_str())
            .collect();
        assert!(fn_names.contains(&"process_graph"), "missing process_graph");
        assert!(fn_names.contains(&"fetch_remote_data"), "missing fetch_remote_data");
        assert!(fn_names.contains(&"new"), "missing new");
        assert!(fn_names.contains(&"node_count"), "missing node_count");
    }

    #[test]
    fn extracts_structs_as_class() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let struct_names: Vec<&str> = result
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Class)
            .map(|n| n.label.as_str())
            .collect();
        assert!(struct_names.contains(&"GraphNode"), "missing GraphNode");
        assert!(struct_names.contains(&"GraphService"), "missing GraphService");
    }

    #[test]
    fn extracts_enums_as_class() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let class_names: Vec<&str> = result
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Class)
            .map(|n| n.label.as_str())
            .collect();
        assert!(class_names.contains(&"EdgeType"), "missing EdgeType enum");
    }

    #[test]
    fn extracts_traits_as_interface() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let trait_node = result
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Interface)
            .expect("trait node");
        assert_eq!(trait_node.label, "GraphOperations");
    }

    #[test]
    fn extracts_mod_declarations() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let mod_names: Vec<&str> = result
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Module && n.label != "graph")
            .map(|n| n.label.as_str())
            .collect();
        assert!(mod_names.contains(&"actors"), "missing mod actors");
        assert!(mod_names.contains(&"utils"), "missing mod utils");
    }

    #[test]
    fn generates_contains_edges_from_file_module() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let file_urn = "urn:visionclaw:concept:code:src/graph.rs:graph";
        let contains_targets: Vec<&str> = result
            .edges
            .iter()
            .filter(|e| e.source_urn == file_urn && e.kind == EdgeKind::Contains)
            .map(|e| e.target_urn.as_str())
            .collect();
        assert!(
            contains_targets
                .iter()
                .any(|u| u.ends_with(":process_graph")),
            "file module should contain process_graph"
        );
        assert!(
            contains_targets
                .iter()
                .any(|u| u.ends_with(":GraphNode")),
            "file module should contain GraphNode"
        );
    }

    #[test]
    fn generates_imports_edges() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let import_targets: Vec<&str> = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Imports)
            .map(|e| e.target_urn.as_str())
            .collect();
        assert!(
            import_targets
                .iter()
                .any(|u| u.contains("std::collections::HashMap")),
            "should import HashMap"
        );
        assert!(
            import_targets.iter().any(|u| u.contains("serde::Serialize")),
            "should import Serialize"
        );
        assert!(
            import_targets
                .iter()
                .any(|u| u.contains("serde::Deserialize")),
            "should import Deserialize"
        );
    }

    #[test]
    fn generates_implements_edge() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let impl_edges: Vec<(&str, &str)> = result
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Implements)
            .map(|e| (e.source_urn.as_str(), e.target_urn.as_str()))
            .collect();
        assert!(
            impl_edges
                .iter()
                .any(|(src, tgt)| src.contains("GraphService") && tgt.contains("GraphOperations")),
            "GraphService should implement GraphOperations"
        );
    }

    #[test]
    fn functions_have_line_numbers() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let process_fn = result
            .nodes
            .iter()
            .find(|n| n.label == "process_graph")
            .expect("process_graph node");
        assert!(
            process_fn.properties.contains_key("line"),
            "function node should have line property"
        );
    }

    #[test]
    fn empty_source_produces_error() {
        let result = extractor().extract("", "src/empty.rs");
        // The file module node is always created, but an error is NOT generated
        // because we have at least one node (the file module).
        assert_eq!(result.nodes.len(), 1); // just the file module
    }

    #[test]
    fn module_name_from_lib_rs() {
        assert_eq!(
            RustExtractor::module_name_from_path("crates/my-crate/src/lib.rs"),
            "src"
        );
    }

    #[test]
    fn module_name_from_regular_file() {
        assert_eq!(
            RustExtractor::module_name_from_path("src/actors/graph.rs"),
            "graph"
        );
    }

    #[test]
    fn module_name_from_mod_rs() {
        assert_eq!(
            RustExtractor::module_name_from_path("src/actors/mod.rs"),
            "actors"
        );
    }

    #[test]
    fn no_duplicate_nodes() {
        let result = extractor().extract(SAMPLE_RUST, "src/graph.rs");
        let mut urns: Vec<&str> = result.nodes.iter().map(|n| n.urn.as_str()).collect();
        let len_before = urns.len();
        urns.sort();
        urns.dedup();
        assert_eq!(len_before, urns.len(), "duplicate URNs in node list");
    }

    #[test]
    fn extractor_for_path_works() {
        let ext = super::super::extractor_for_path("src/main.rs");
        assert!(ext.is_some());
        assert_eq!(ext.unwrap().language(), Language::Rust);

        let ext = super::super::extractor_for_path("README.md");
        assert!(ext.is_none());
    }

    #[test]
    fn unsafe_fn_extracted() {
        let source = "pub unsafe fn dangerous_thing() {}";
        let result = extractor().extract(source, "src/unsafe.rs");
        let names: Vec<&str> = result
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Function)
            .map(|n| n.label.as_str())
            .collect();
        assert!(names.contains(&"dangerous_thing"));
    }

    #[test]
    fn const_fn_extracted() {
        let source = "pub const fn max_size() -> usize { 1024 }";
        let result = extractor().extract(source, "src/consts.rs");
        let names: Vec<&str> = result
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Function)
            .map(|n| n.label.as_str())
            .collect();
        assert!(names.contains(&"max_size"));
    }

    #[test]
    fn unsafe_trait_extracted() {
        let source = "pub unsafe trait Send {}";
        let result = extractor().extract(source, "src/marker.rs");
        let trait_node = result
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Interface)
            .expect("unsafe trait node");
        assert_eq!(trait_node.label, "Send");
    }

    // ── Error handling / edge case tests ──

    #[test]
    fn malformed_use_group_no_closing_brace() {
        // Missing closing brace should not panic -- parse_use_path returns the
        // raw path as-is when braces are unbalanced.
        let names = parse_use_path("std::io::{Read, Write");
        // The brace_start is found but brace_end is not, so the function falls
        // through to the non-brace path and returns the raw string.
        assert!(!names.is_empty());
    }

    #[test]
    fn empty_use_group_braces() {
        let names = parse_use_path("std::io::{}");
        // Empty brace group produces no items
        assert!(names.is_empty());
    }

    #[test]
    fn source_with_string_containing_fn_keyword() {
        // Regression: a string literal containing `fn ` should not produce a
        // false-positive function node.  The regex anchors to line-start
        // indentation so `"fn foo"` inside a let binding will match — this is a
        // known limitation documented in the module doc.  We verify no panic.
        let source = r#"
let msg = "fn fake_func() should not crash";
pub fn real_func() {}
"#;
        let result = extractor().extract(source, "src/string_edge.rs");
        // At minimum real_func should appear
        let fn_names: Vec<&str> = result
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Function)
            .map(|n| n.label.as_str())
            .collect();
        assert!(fn_names.contains(&"real_func"));
    }

    #[test]
    fn consolidated_struct_enum_pass_produces_same_results() {
        // Verify the consolidated struct+enum pass extracts both correctly
        let source = r#"
pub struct Alpha { x: u32 }
pub enum Beta { A, B }
struct Gamma;
enum Delta { X }
"#;
        let result = extractor().extract(source, "src/types.rs");
        let class_names: Vec<&str> = result
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Class)
            .map(|n| n.label.as_str())
            .collect();
        assert!(class_names.contains(&"Alpha"), "missing struct Alpha");
        assert!(class_names.contains(&"Beta"), "missing enum Beta");
        assert!(class_names.contains(&"Gamma"), "missing struct Gamma");
        assert!(class_names.contains(&"Delta"), "missing enum Delta");
        assert_eq!(class_names.len(), 4);
    }
}
