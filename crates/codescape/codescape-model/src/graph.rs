//! Nodes, edges and snapshots of a code graph.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Semantic class of a node. `Other` keeps graphicle's string so nothing is lost.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A directory super-node (LOD tier 0).
    Directory,
    /// A source file.
    File,
    /// A module or namespace.
    Module,
    /// A class.
    Class,
    /// A struct or record.
    Struct,
    /// An enum.
    Enum,
    /// A trait, interface or protocol.
    Interface,
    /// A free function.
    Function,
    /// A method.
    Method,
    /// A variable, field or property.
    Variable,
    /// A constant.
    Constant,
    /// A type alias.
    TypeAlias,
    /// A prose document (Markdown, PDF…); off by default in ingest.
    Document,
    /// A symbol outside the repository (external stub).
    External,
    /// Anything else, carrying graphicle's own type string.
    Other(Arc<str>),
}

impl NodeKind {
    /// Map a graphicle `type` attribute to a kind.
    pub fn from_graphicle(ty: &str) -> Self {
        match ty {
            "directory" | "dir" | "package" => Self::Directory,
            "file" => Self::File,
            "module" | "namespace" => Self::Module,
            "class" => Self::Class,
            "struct" | "record" => Self::Struct,
            "enum" => Self::Enum,
            "trait" | "interface" | "protocol" => Self::Interface,
            "function" | "func" | "fn" => Self::Function,
            "method" => Self::Method,
            "variable" | "field" | "property" | "attribute" => Self::Variable,
            "constant" | "const" | "static" => Self::Constant,
            "type" | "type_alias" | "typedef" => Self::TypeAlias,
            "document" | "section" | "paper" | "image" => Self::Document,
            "external" | "stub" => Self::External,
            other => Self::Other(Arc::from(other)),
        }
    }

    /// Stable short name for the wire (`node_type` in `initialGraphLoad`).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Module => "module",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Function => "function",
            Self::Method => "method",
            Self::Variable => "variable",
            Self::Constant => "constant",
            Self::TypeAlias => "type_alias",
            Self::Document => "document",
            Self::External => "external",
            Self::Other(s) => s,
        }
    }

    /// Whether this kind is a container (directory or file) rather than a symbol.
    pub fn is_container(&self) -> bool {
        matches!(self, Self::Directory | Self::File)
    }
}

/// Source language, by file extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    /// Rust.
    Rust,
    /// TypeScript (including TSX).
    TypeScript,
    /// JavaScript (including JSX, MJS, CJS).
    JavaScript,
    /// Python.
    Python,
    /// Go.
    Go,
    /// GDScript.
    GdScript,
    /// C or C++.
    Cpp,
    /// CUDA.
    Cuda,
    /// Shell.
    Shell,
    /// Nix.
    Nix,
    /// Markdown.
    Markdown,
    /// Anything else or unknown.
    Other,
}

impl Lang {
    /// Guess from a path's extension.
    pub fn from_path(path: &str) -> Self {
        let ext = path.rsplit('.').next().unwrap_or("");
        match ext {
            "rs" => Self::Rust,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "py" | "pyi" => Self::Python,
            "go" => Self::Go,
            "gd" => Self::GdScript,
            "c" | "cc" | "cpp" | "cxx" | "h" | "hpp" | "hh" => Self::Cpp,
            "cu" | "cuh" => Self::Cuda,
            "sh" | "bash" | "fish" | "zsh" => Self::Shell,
            "nix" => Self::Nix,
            "md" | "mdx" | "markdown" => Self::Markdown,
            _ => Self::Other,
        }
    }

    /// Stable short name (`metadata.domain` on the wire).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Go => "go",
            Self::GdScript => "gdscript",
            Self::Cpp => "cpp",
            Self::Cuda => "cuda",
            Self::Shell => "shell",
            Self::Nix => "nix",
            Self::Markdown => "markdown",
            Self::Other => "other",
        }
    }
}

/// Level-of-detail tier a node belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Crate / package / top-level directory super-nodes (hundreds).
    L0,
    /// Files (thousands).
    L1,
    /// Symbols (up to hundreds of thousands).
    L2,
}

/// A 1-based inclusive line span inside the node's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Span {
    /// First line.
    pub start_line: u32,
    /// Last line.
    pub end_line: u32,
}

/// One node of the code graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeNode {
    /// Wire id without flag bits (26-bit, from [`crate::IdInterner`]).
    pub id: u32,
    /// Graphicle's stable node id string (`path::Symbol`), the interning key.
    pub key: Arc<str>,
    /// Semantic class.
    pub kind: NodeKind,
    /// Source language.
    pub lang: Lang,
    /// Display label (symbol name or file basename).
    pub label: Arc<str>,
    /// Repository-relative path of the containing file, when known.
    pub path: Option<Arc<str>>,
    /// Line span in that file, when known.
    pub span: Option<Span>,
    /// Interned id of the structural parent (file for a symbol, directory for a file), 0 = none.
    pub parent: u32,
    /// Leiden community (stable-matched); 0 = unassigned.
    pub community: u32,
    /// Structural cluster: crate/package/top-level directory index.
    pub cluster: u32,
    /// Importance in 0..1 (betweenness today, PageRank optionally later).
    pub importance: f32,
    /// LOD tier.
    pub tier: Tier,
    /// Lines of code when known (files), else 0.
    pub size: u32,
}

/// Relation kind of an edge, mirroring graphicle's relation strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// Container → member.
    Contains,
    /// Class → method.
    Method,
    /// Caller → callee.
    Calls,
    /// File → imported module.
    Imports,
    /// File → imported symbol.
    ImportsFrom,
    /// Subclass → superclass.
    Inherits,
    /// Type → interface.
    Implements,
    /// Generic reference.
    References,
    /// Definer → defined.
    Defines,
    /// Extension relation.
    Extends,
    /// Enum → variant.
    CaseOf,
    /// Anything else.
    Other(Arc<str>),
}

impl Relation {
    /// Map a graphicle relation string.
    pub fn from_graphicle(rel: &str) -> Self {
        match rel {
            "contains" => Self::Contains,
            "method" => Self::Method,
            "calls" => Self::Calls,
            "imports" => Self::Imports,
            "imports_from" => Self::ImportsFrom,
            "inherits" => Self::Inherits,
            "implements" => Self::Implements,
            "references" => Self::References,
            "defines" => Self::Defines,
            "extends" => Self::Extends,
            "case_of" => Self::CaseOf,
            other => Self::Other(Arc::from(other)),
        }
    }

    /// Stable short name (`edge_type` on the wire).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Contains => "contains",
            Self::Method => "method",
            Self::Calls => "calls",
            Self::Imports => "imports",
            Self::ImportsFrom => "imports_from",
            Self::Inherits => "inherits",
            Self::Implements => "implements",
            Self::References => "references",
            Self::Defines => "defines",
            Self::Extends => "extends",
            Self::CaseOf => "case_of",
            Self::Other(s) => s,
        }
    }

    /// Whether the relation is structural (containment) rather than semantic.
    pub fn is_structural(&self) -> bool {
        matches!(
            self,
            Self::Contains | Self::Method | Self::CaseOf | Self::Defines
        )
    }
}

/// How sure the extractor was about an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    /// Read directly from syntax.
    #[default]
    Extracted,
    /// Resolved by heuristics.
    Inferred,
    /// Could not be disambiguated.
    Ambiguous,
}

/// One edge of the code graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeEdge {
    /// Source wire id.
    pub source: u32,
    /// Target wire id.
    pub target: u32,
    /// Relation kind.
    pub relation: Relation,
    /// Layout weight (structural and intra-community edges are stronger).
    pub weight: f32,
    /// Extraction confidence.
    pub confidence: Confidence,
}

/// A complete, self-consistent graph at one revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct GraphSnapshot {
    /// Repository label (`visionclaw`, `agentbox`…).
    pub repo: Arc<str>,
    /// Interner epoch the ids belong to.
    pub epoch: u32,
    /// Monotonic revision; deltas move it forward by one.
    pub revision: u64,
    /// Unix milliseconds when this revision was produced.
    pub generated_at_ms: u64,
    /// Nodes, in any order; ids are unique.
    pub nodes: Vec<CodeNode>,
    /// Edges between `nodes`.
    pub edges: Vec<CodeEdge>,
}

impl GraphSnapshot {
    /// Position of each id in `nodes`.
    pub fn index(&self) -> HashMap<u32, usize> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect()
    }

    /// The node with `id`, scanning linearly (use [`GraphSnapshot::index`] in loops).
    pub fn node(&self, id: u32) -> Option<&CodeNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Count of nodes in each tier.
    pub fn tier_counts(&self) -> [usize; 3] {
        let mut c = [0usize; 3];
        for n in &self.nodes {
            c[n.tier as usize] += 1;
        }
        c
    }

    /// Check the invariants: unique ids, every edge endpoint present, parents present.
    pub fn validate(&self) -> Result<(), String> {
        let idx = self.index();
        if idx.len() != self.nodes.len() {
            return Err("duplicate node ids".into());
        }
        for n in &self.nodes {
            if n.id == 0 || n.id & !super::ids::NODE_ID_MASK != 0 {
                return Err(format!("node id {} outside the 26-bit space", n.id));
            }
            if n.parent != 0 && !idx.contains_key(&n.parent) {
                return Err(format!("node {} has missing parent {}", n.id, n.parent));
            }
        }
        for e in &self.edges {
            if !idx.contains_key(&e.source) || !idx.contains_key(&e.target) {
                return Err(format!(
                    "edge {}->{} has a missing endpoint",
                    e.source, e.target
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u32, parent: u32) -> CodeNode {
        CodeNode {
            id,
            key: Arc::from(format!("k{id}")),
            kind: NodeKind::Function,
            lang: Lang::Rust,
            label: Arc::from("f"),
            path: None,
            span: None,
            parent,
            community: 0,
            cluster: 0,
            importance: 0.0,
            tier: Tier::L2,
            size: 0,
        }
    }

    #[test]
    fn kinds_and_relations_round_trip_strings() {
        for s in ["function", "class", "file", "weird_thing"] {
            assert_eq!(NodeKind::from_graphicle(s).as_str(), s);
        }
        for s in ["calls", "imports_from", "case_of", "custom"] {
            assert_eq!(Relation::from_graphicle(s).as_str(), s);
        }
        assert_eq!(Lang::from_path("a/b/c.tsx"), Lang::TypeScript);
        assert_eq!(Lang::from_path("Makefile"), Lang::Other);
    }

    #[test]
    fn validate_catches_broken_graphs() {
        let mut g = GraphSnapshot {
            nodes: vec![node(1, 0), node(2, 1)],
            ..Default::default()
        };
        assert!(g.validate().is_ok());
        g.edges.push(CodeEdge {
            source: 1,
            target: 9,
            relation: Relation::Calls,
            weight: 1.0,
            confidence: Confidence::Extracted,
        });
        assert!(g.validate().unwrap_err().contains("missing endpoint"));
        g.edges.clear();
        g.nodes.push(node(1, 0));
        assert_eq!(g.validate().unwrap_err(), "duplicate node ids");
    }
}
