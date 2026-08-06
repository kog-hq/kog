use crate::discover::discover;
use crate::extractor::{Extractor, Resolution};
use crate::model::{Edge, EdgeKind, Failure, Graph, Node, Stats};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

/// Project-relative, slash-separated identity for a file.
fn node_id(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Same as [`node_id`], but falls back to canonicalising `path` first when a
/// direct `strip_prefix` against the (already canonical) `root` fails. This
/// covers paths that were built by an `Extractor` against its own idea of
/// root (e.g. a tsconfig index) rather than the canonical root `build_graph`
/// uses for its file nodes.
fn node_id_lenient(root: &Path, path: &Path) -> Option<String> {
    node_id(root, path).or_else(|| node_id(root, &path.canonicalize().ok()?))
}

/// Scan a project into a graph. Never panics on a bad file: it is recorded in
/// `stats.failures` and the scan continues.
pub fn build_graph(root: &Path, extractor: &dyn Extractor) -> Graph {
    // `discover` canonicalises its own root internally; do the same here so
    // that `node_id` strips the exact prefix `discover` used. Without this,
    // a non-canonical `root` (e.g. one containing a `..` component) makes
    // every `strip_prefix` below fail silently, and every node is skipped.
    let root: PathBuf = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root: &Path = &root;

    let files = discover(root, extractor.extensions());

    let mut stats = Stats {
        files_discovered: files.len(),
        ..Default::default()
    };
    let mut nodes: Vec<Node> = Vec::new();
    let mut edges: HashSet<Edge> = HashSet::new();
    let mut external_names: BTreeSet<String> = BTreeSet::new();
    // A specifier may resolve to a file that was filtered out of the walk.
    let known: HashSet<String> = files.iter().filter_map(|p| node_id(root, p)).collect();

    for file in &files {
        let id = match node_id(root, file) {
            Some(id) => id,
            None => continue,
        };

        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                stats.failures.push(Failure {
                    path: id.clone(),
                    reason: format!("read failed: {e}"),
                });
                continue;
            }
        };

        let specifiers = match extractor.extract(&source) {
            Ok(s) => s,
            Err(e) => {
                stats.failures.push(Failure {
                    path: id.clone(),
                    reason: format!("extract failed: {e}"),
                });
                continue;
            }
        };
        stats.files_parsed += 1;

        let mut node_externals: BTreeSet<String> = BTreeSet::new();

        for specifier in &specifiers {
            stats.specifiers_total += 1;
            match extractor.resolve(&specifier.raw, file) {
                Resolution::External(package) => {
                    stats.external_specifiers += 1;
                    external_names.insert(package.clone());
                    node_externals.insert(package);
                }
                Resolution::Internal(target) => {
                    stats.specifiers_internal += 1;
                    match node_id(root, &target).filter(|t| known.contains(t)) {
                        Some(target_id) => {
                            stats.resolved += 1;
                            if target_id != id {
                                edges.insert(Edge {
                                    source: id.clone(),
                                    target: target_id,
                                    kind: EdgeKind::Import,
                                });
                            }
                        }
                        // Resolved on disk but outside the scanned set.
                        None => stats.unresolved += 1,
                    }
                }
                Resolution::Unresolved => {
                    stats.specifiers_internal += 1;
                    stats.unresolved += 1;
                }
            }
        }

        nodes.push(Node {
            id: id.clone(),
            path: id,
            lang: extractor.lang().to_string(),
            loc: source.lines().count(),
            external_deps: node_externals.into_iter().collect(),
        });
    }

    // A config the extractor could not use (e.g. an unreadable or malformed
    // tsconfig) degrades resolution for a whole subtree; design doc §7
    // demands that never disappear silently. The reason is prefixed so it
    // reads distinctly from a source-file failure.
    for skipped in extractor.skipped_configs() {
        let path = node_id_lenient(root, &skipped.path)
            .unwrap_or_else(|| skipped.path.to_string_lossy().into_owned());
        stats.failures.push(Failure {
            path,
            reason: format!("tsconfig skipped: {}", skipped.reason),
        });
    }

    stats.external_packages_distinct = external_names.len();
    stats.resolution_rate = stats.resolution_rate();

    let mut edges: Vec<Edge> = edges.into_iter().collect();
    edges.sort_by(|a, b| (&a.source, &a.target).cmp(&(&b.source, &b.target)));
    nodes.sort_by(|a, b| a.id.cmp(&b.id));

    Graph {
        nodes,
        edges,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use crate::extractors::TypeScriptExtractor;
    use crate::model::Graph;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &TempDir, rel: &str, body: &str) {
        let path = dir.path().join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn build(dir: &TempDir) -> Graph {
        let extractor = TypeScriptExtractor::new(dir.path());
        crate::build_graph(dir.path(), &extractor)
    }

    #[test]
    fn every_discovered_file_becomes_a_node() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import b from "./b";"#);
        write(&dir, "src/b.ts", "export const b = 1;");
        let graph = build(&dir);
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn node_ids_are_project_relative_and_slash_separated() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/deep/a.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.nodes[0].id, "src/deep/a.ts");
    }

    #[test]
    fn external_packages_land_on_the_node_not_in_the_edges() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import React from "react";"#);
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 0);
        assert_eq!(graph.nodes[0].external_deps, vec!["react".to_string()]);
    }

    #[test]
    fn duplicate_edges_are_collapsed() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/a.ts",
            r#"import { x } from "./b";
import { y } from "./b";"#,
        );
        write(&dir, "src/b.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn stats_count_internal_and_external_separately() {
        let dir = TempDir::new().unwrap();
        write(
            &dir,
            "src/a.ts",
            r#"import React from "react";
import b from "./b";
import ghost from "./ghost";"#,
        );
        write(&dir, "src/b.ts", "");
        let graph = build(&dir);
        assert_eq!(graph.stats.specifiers_total, 3);
        assert_eq!(graph.stats.specifiers_internal, 2);
        assert_eq!(graph.stats.resolved, 1);
        assert_eq!(graph.stats.unresolved, 1);
        assert_eq!(graph.stats.external_specifiers, 1);
        assert_eq!(graph.stats.external_packages_distinct, 1);
        assert!((graph.stats.resolution_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn loc_counts_the_lines_of_each_file() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", "one\ntwo\nthree");
        let graph = build(&dir);
        assert_eq!(graph.nodes[0].loc, 3);
    }

    #[test]
    fn an_edge_pointing_outside_the_project_is_dropped_and_counted() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import x from "../../outside/x";"#);
        let graph = build(&dir);
        assert_eq!(graph.edges.len(), 0);
    }

    #[test]
    fn nodes_are_sorted_so_runs_are_reproducible() {
        let dir = TempDir::new().unwrap();
        write(&dir, "src/z.ts", "");
        write(&dir, "src/a.ts", "");
        let graph = build(&dir);
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["src/a.ts", "src/z.ts"]);
    }

    // --- Extra coverage beyond the brief ---

    #[test]
    fn build_graph_handles_a_non_canonical_root() {
        // `discover` canonicalises its root internally. If `build_graph`
        // does not canonicalise the same way before computing node ids, the
        // `strip_prefix` inside `node_id` silently fails against the
        // canonical paths `discover` hands back, and every node is skipped.
        let dir = TempDir::new().unwrap();
        write(&dir, "src/a.ts", r#"import b from "./b";"#);
        write(&dir, "src/b.ts", "export const b = 1;");

        // Force a `..` component into the root so a naive `strip_prefix`
        // against the raw (non-canonical) root would fail.
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        let noncanonical_root = subdir.join("..");
        assert!(
            noncanonical_root
                .components()
                .any(|c| c == std::path::Component::ParentDir),
            "test setup must actually produce a `..` component"
        );

        let extractor = TypeScriptExtractor::new(&noncanonical_root);
        let graph = crate::build_graph(&noncanonical_root, &extractor);

        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
        let ids: Vec<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]
    fn a_skipped_tsconfig_is_recorded_as_a_failure() {
        // Design doc §7: a tsconfig that could not be read or parsed
        // degrades alias resolution for a whole subtree, and that must
        // never disappear silently.
        let dir = TempDir::new().unwrap();
        write(&dir, "tsconfig.json", "{ this is not json at all ");
        write(&dir, "src/a.ts", "");
        let graph = build(&dir);

        let failure = graph
            .stats
            .failures
            .iter()
            .find(|f| f.path == "tsconfig.json");
        assert!(
            failure.is_some(),
            "expected a failure entry for the skipped tsconfig.json, got {:?}",
            graph.stats.failures
        );
        let failure = failure.unwrap();
        assert!(
            failure.reason.starts_with("tsconfig skipped: "),
            "reason should be prefixed to distinguish it from a source-file \
             failure, got {:?}",
            failure.reason
        );
        assert!(!failure.reason["tsconfig skipped: ".len()..].is_empty());
    }
}
