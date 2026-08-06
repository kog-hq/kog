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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub files_discovered: usize,
    pub files_parsed: usize,
    pub specifiers_total: usize,
    pub specifiers_internal: usize,
    pub resolved: usize,
    pub unresolved: usize,
    /// Serialised snapshot of the resolution rate. The `resolution_rate()` method is the
    /// source of truth; the graph assembler must populate this field from the method.
    pub resolution_rate: f64,
    pub external_specifiers: usize,
    pub external_packages_distinct: usize,
    pub failures: Vec<Failure>,
}

impl Stats {
    /// Resolved over internal specifiers. External specifiers are excluded:
    /// `import react` has no file to point at.
    pub fn resolution_rate(&self) -> f64 {
        if self.specifiers_internal == 0 {
            return 1.0;
        }
        self.resolved as f64 / self.specifiers_internal as f64
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
            unresolved: 20,
            resolution_rate: 0.95,
            external_specifiers: 100,
            external_packages_distinct: 15,
            failures: vec![Failure {
                path: "src/bad.ts".into(),
                reason: "parse error".into(),
            }],
        };
        let json = serde_json::to_value(&stats).unwrap();

        // Assert all ten Stats fields are present with correct keys
        assert_eq!(json["files_discovered"], 42);
        assert_eq!(json["files_parsed"], 40);
        assert_eq!(json["specifiers_total"], 500);
        assert_eq!(json["specifiers_internal"], 400);
        assert_eq!(json["resolved"], 380);
        assert_eq!(json["unresolved"], 20);
        assert_eq!(json["resolution_rate"], 0.95);
        assert_eq!(json["external_specifiers"], 100);
        assert_eq!(json["external_packages_distinct"], 15);

        // Assert Failure inside failures vector serialises with correct keys
        assert_eq!(json["failures"][0]["path"], "src/bad.ts");
        assert_eq!(json["failures"][0]["reason"], "parse error");
    }
}
