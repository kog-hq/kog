use serde::{Deserialize, Serialize};

/// One source file in the scanned project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Project-relative path, also used as the graph identity.
    pub id: String,
    pub path: String,
    pub lang: String,
    pub loc: usize,
    /// External package names this file imports, never graph edges.
    pub external_deps: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Import,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    pub kind: EdgeKind,
}

/// A file that could not be read or parsed. Never fatal, always reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub path: String,
    pub reason: String,
}

/// Why one specifier did not become a graph edge. Distinct from a
/// [`Failure`]: nothing here means the parser or resolver malfunctioned —
/// see the field on each variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticKind {
    /// The specifier looks internal, but either no file was found for it
    /// anywhere on disk, or a file was found and the tool itself failed to
    /// read or parse it (our failure, not the target's) — a genuinely
    /// broken import either way. Depresses the resolution rate.
    Unresolved,
    /// The specifier resolved to a real file, but that file falls outside
    /// the scanned node set (gitignored, inside an always-skipped
    /// directory, or an extension this extractor does not claim) — e.g. a
    /// generated Prisma client. A deliberate exclusion, not a parser
    /// failure; left out of the resolution rate's denominator.
    Excluded,
}

impl std::fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticKind::Unresolved => write!(f, "unresolved"),
            DiagnosticKind::Excluded => write!(f, "excluded"),
        }
    }
}

/// One specifier that did not become an edge, identified precisely enough
/// to find and fix in the source. Design doc §7: a count alone is not
/// auditable, only the identity of what failed is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The importing file's id (project-relative), i.e. `Node::id`.
    pub path: String,
    /// One-based line number of the import statement.
    pub line: usize,
    /// The raw specifier text, exactly as written in the source.
    pub specifier: String,
    pub kind: DiagnosticKind,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub files_discovered: usize,
    pub files_parsed: usize,
    pub specifiers_total: usize,
    /// Specifiers whose target looks internal to the project (a relative
    /// import, a resolved alias, or a workspace package): the sum of
    /// `resolved`, `unresolved`, and `excluded`.
    pub specifiers_internal: usize,
    /// Resolved to a real file inside the scanned node set: this specifier
    /// became (or would have become, after edge dedup) a graph edge.
    pub resolved: usize,
    /// No file was found for this specifier at all. See
    /// [`DiagnosticKind::Unresolved`]; each occurrence is also recorded in
    /// `diagnostics` (subject to the cap documented there).
    pub unresolved: usize,
    /// Resolved to a real file that falls outside the scanned node set. See
    /// [`DiagnosticKind::Excluded`]; each occurrence is also recorded in
    /// `diagnostics` (subject to the cap documented there).
    pub excluded: usize,
    /// Serialised snapshot of the resolution rate. The `resolution_rate()` method is the
    /// source of truth; the graph assembler must populate this field from the method.
    pub resolution_rate: f64,
    pub external_specifiers: usize,
    pub external_packages_distinct: usize,
    pub failures: Vec<Failure>,
    /// One entry per `unresolved` or `excluded` specifier, capped at
    /// [`MAX_DIAGNOSTICS`] so a huge broken repo cannot produce an unbounded
    /// `graph.json`. `unresolved` and `excluded` above always hold the true
    /// totals even when this list was capped.
    pub diagnostics: Vec<Diagnostic>,
}

/// Cap on `Stats::diagnostics`. Chosen to comfortably cover a real broken
/// subtree (the acceptance target's own worst case is under 100) while
/// bounding `graph.json` size against a pathological repo with thousands of
/// broken imports; `unresolved`/`excluded` remain exact regardless.
pub const MAX_DIAGNOSTICS: usize = 500;

impl Stats {
    /// Resolved over "in-scope" internal specifiers: internal specifiers
    /// minus those `excluded` (resolved to a real file outside the scanned
    /// set). `excluded` is left out of the denominator for the same reason
    /// external specifiers already are — the resolver worked correctly, the
    /// target is just deliberately out of scope, so counting it against the
    /// rate would understate resolver quality rather than measure it.
    pub fn resolution_rate(&self) -> f64 {
        let denominator = self.specifiers_internal.saturating_sub(self.excluded);
        if denominator == 0 {
            return 1.0;
        }
        self.resolved as f64 / denominator as f64
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub stats: Stats,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_computes_the_resolution_rate_over_internal_specifiers_only() {
        let stats = Stats {
            specifiers_internal: 3209,
            resolved: 3140,
            unresolved: 69,
            ..Default::default()
        };
        assert!((stats.resolution_rate() - 0.9785).abs() < 1e-4);
    }

    #[test]
    fn resolution_rate_is_one_when_there_is_nothing_to_resolve() {
        let stats = Stats::default();
        assert_eq!(stats.resolution_rate(), 1.0);
    }

    #[test]
    fn resolution_rate_excludes_out_of_scope_specifiers_from_the_denominator() {
        // 8 resolved out of 10 internal specifiers, 1 of which is merely
        // `excluded` (a real file outside the scanned set, not a parser
        // failure). The old formula (resolved / specifiers_internal) would
        // give 8/10 = 0.8; the correct formula removes the exclusion from
        // the denominator entirely, exactly like an external specifier:
        // 8 resolved out of (10 - 1) = 9 in-scope internal specifiers.
        let stats = Stats {
            specifiers_internal: 10,
            resolved: 8,
            unresolved: 1,
            excluded: 1,
            ..Default::default()
        };
        assert!((stats.resolution_rate() - (8.0 / 9.0)).abs() < 1e-9);
    }

    #[test]
    fn a_graph_serialises_to_the_documented_shape() {
        let graph = Graph {
            nodes: vec![Node {
                id: "src/a.ts".into(),
                path: "src/a.ts".into(),
                lang: "typescript".into(),
                loc: 12,
                external_deps: vec!["react".into()],
            }],
            edges: vec![Edge {
                source: "src/a.ts".into(),
                target: "src/b.ts".into(),
                kind: EdgeKind::Import,
            }],
            stats: Stats::default(),
        };
        let json = serde_json::to_value(&graph).unwrap();
        assert_eq!(json["nodes"][0]["external_deps"][0], "react");
        assert_eq!(json["edges"][0]["kind"], "import");
    }

    #[test]
    fn stats_all_fields_serialise_with_documented_keys() {
        let stats = Stats {
            files_discovered: 42,
            files_parsed: 40,
            specifiers_total: 500,
            specifiers_internal: 400,
            resolved: 380,
            unresolved: 15,
            excluded: 5,
            resolution_rate: 0.95,
            external_specifiers: 100,
            external_packages_distinct: 15,
            failures: vec![Failure {
                path: "src/bad.ts".into(),
                reason: "parse error".into(),
            }],
            diagnostics: vec![Diagnostic {
                path: "src/a.ts".into(),
                line: 3,
                specifier: "./ghost".into(),
                kind: DiagnosticKind::Unresolved,
            }],
        };
        let json = serde_json::to_value(&stats).unwrap();

        // Assert every Stats field is present with correct keys
        assert_eq!(json["files_discovered"], 42);
        assert_eq!(json["files_parsed"], 40);
        assert_eq!(json["specifiers_total"], 500);
        assert_eq!(json["specifiers_internal"], 400);
        assert_eq!(json["resolved"], 380);
        assert_eq!(json["unresolved"], 15);
        assert_eq!(json["excluded"], 5);
        assert_eq!(json["resolution_rate"], 0.95);
        assert_eq!(json["external_specifiers"], 100);
        assert_eq!(json["external_packages_distinct"], 15);

        // Assert Failure inside failures vector serialises with correct keys
        assert_eq!(json["failures"][0]["path"], "src/bad.ts");
        assert_eq!(json["failures"][0]["reason"], "parse error");

        // Assert Diagnostic inside diagnostics vector serialises with
        // correct keys, and its kind renders as the documented snake_case.
        assert_eq!(json["diagnostics"][0]["path"], "src/a.ts");
        assert_eq!(json["diagnostics"][0]["line"], 3);
        assert_eq!(json["diagnostics"][0]["specifier"], "./ghost");
        assert_eq!(json["diagnostics"][0]["kind"], "unresolved");
    }
}
